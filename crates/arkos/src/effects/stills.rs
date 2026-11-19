//! Stills of a wash for designing an effect off the screen: a realistic window (the title bar
//! with its words and icons, a sidebar with a selected row and its names ending part way down,
//! two panes side by side, the left focused and at work with its terminal rows, the right
//! waiting) drawn by a built-in effect's wash in a theme, written as a PPM picture. Run by hand:
//! `ARKOS_STILLS=<dir> cargo test -p arkos effects::stills -- --ignored`, then look at the
//! pictures. Nothing here runs in the suite.

#[cfg(test)]
mod tests {
    use crate::effects::blocks::{blocks, HighlightState, Kind, Mark, Scene, Shape};
    use crate::tree::Rect;

    const W: u32 = 1000;
    const H: u32 = 640;
    const SIDE: f32 = 240.0;

    /// The window's blocks for an effect whose manifest keeps `gap` round the panes, at
    /// `sidebar_focus` (the Cmd+0 pane's glow).
    fn window(gap: f32, sidebar_focus: f32) -> Vec<crate::effects::blocks::Block> {
        window_to(gap, sidebar_focus, crate::chrome::TITLE_BAR_H + 300.0)
    }

    /// `window`, its sidebar's names ending at `names_end`.
    fn window_to(gap: f32, sidebar_focus: f32, names_end: f32) -> Vec<crate::effects::blocks::Block> {
        let top = crate::chrome::TITLE_BAR_H;
        let tab = crate::pane_view::TAB_BAR_H;
        // As the app lays it out: the gap as the gutter beside the sidebar (reported as the
        // binder), the effect's edge margin off the bar and the window's edges.
        let edge = edge();
        let content = Rect { x: SIDE + gap, y: top + edge, w: W as f32 - SIDE - gap - edge, h: H as f32 - top - 2.0 * edge };
        let pw = (content.w - gap) / 2.0;
        let left = Rect { x: content.x, y: content.y, w: pw, h: content.h };
        let right = Rect { x: content.x + pw + gap, y: content.y, w: pw, h: content.h };
        let panes = [(1u64, left), (2u64, right)];
        let body = |p: Rect| Rect { x: p.x, y: p.y + tab, w: p.w, h: p.h - tab };
        let shapes = [
            Shape { key: 1, rect: Rect { x: 80.0, y: 9.0, w: 260.0, h: 18.0 }, kind: Kind::TitleGroup, pitch: 0.0 },
            Shape { key: 2, rect: Rect { x: W as f32 - 80.0, y: 7.0, w: 60.0, h: 22.0 }, kind: Kind::TitleGroup, pitch: 0.0 },
            Shape { key: 7, rect: Rect { x: left.x, y: left.y, w: 120.0, h: tab }, kind: Kind::ActiveTab, pitch: 0.0 },
            Shape { key: 9, rect: Rect { x: right.x, y: right.y, w: 120.0, h: tab }, kind: Kind::ActiveTab, pitch: 0.0 },
            Shape { key: 8, rect: { let b = body(left); Rect { x: b.x + 6.0, y: b.y + 6.0, w: b.w - 12.0, h: b.h - 12.0 } }, kind: Kind::Rows, pitch: 18.0 },
        ];
        let row = Mark { key: 99, rect: Rect { x: 8.0, y: top + 120.0, w: SIDE - 16.0, h: 34.0 }, kind: Kind::RowHighlight, state: HighlightState::Selected };
        let strokes = [(row, 1.0)];
        let (busy, focus, waiting) = ([1u64], [(1u64, 1.0f32)], [2u64]);
        let scene = Scene { edge_gap: edge, sidebar_margin: edge, focus: &focus, sidebar_focus, waiting: &waiting, ..crate::effects::active::scene(content, gap, Some(top + 330.0), Some(names_end), &panes, &busy, &shapes, &strokes) };
        let manifest = crate::effects::manifest::parse(crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().manifest).unwrap();
        blocks(&scene, manifest.ink_borders)
    }

    fn write_ppm(path: &std::path::Path, frame: &[crate::palette::Rgb]) {
        let mut out = format!("P6\n{W} {H}\n255\n").into_bytes();
        for c in frame {
            out.extend([c.r, c.g, c.b]);
        }
        std::fs::write(path, out).unwrap();
    }

    #[test]
    #[ignore]
    fn stills() {
        let Ok(dir) = std::env::var("ARKOS_STILLS") else { return };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let t: f32 = std::env::var("ARKOS_STILLS_T").ok().and_then(|s| s.parse().ok()).unwrap_or(2.5);
        // `ARKOS_STILLS_PAIRS=effect:theme,...` draws those instead of the runic and overworld ones.
        let pairs: Vec<(String, String)> = std::env::var("ARKOS_STILLS_PAIRS")
            .ok()
            .map(|v| v.split(',').filter_map(|p| p.split_once(':')).map(|(e, t)| (e.to_string(), t.to_string())).collect())
            .unwrap_or_else(|| [("runic", "tokyo-rain"), ("runic", "matrix"), ("overworld", "timo-purple")].map(|(e, t)| (e.to_string(), t.to_string())).to_vec());
        for (effect, theme) in pairs.iter().map(|(e, t)| (e.as_str(), t.as_str())) {
            let b = crate::effects::BUILTIN.iter().find(|b| b.name == effect).unwrap();
            let m = crate::effects::manifest::parse(b.manifest).unwrap();
            let theme_v = crate::theme::builtin(theme).unwrap();
            for (label, sf) in [("", 0.0), ("-sidebar", 1.0)] {
                let blocks = window(m.gap, sf);
                let frame = crate::effects::bench::render_frame(b.wash, &theme_v, (W, H), t, &blocks);
                write_ppm(&dir.join(format!("{theme}{label}.ppm")), &frame);
            }
        }
    }

    /// How many of the sidebar city's pixels blink (go up, down and up again, or down, up and
    /// down, by more than a quarter of the way from dark to light, over three frames at 60 a
    /// second, which moving light drawn smooth never does), a frame, for each point of the city's
    /// area: the wash drawn at `scale` pixels a point, for a second from `t0`.
    fn city_blinks(scale: f32, t0: f32) -> f32 {
        let b = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap();
        let m = crate::effects::manifest::parse(b.manifest).unwrap();
        let theme = crate::theme::builtin("matrix").unwrap();
        let blocks = window(m.gap, 1.0);
        let w = (W as f32 * scale).round() as u32;
        // The city at the foot of the sidebar, in points.
        let (x0, x1, y0, y1) = (14.0f32, 236.0f32, 400.0f32, 630.0f32);
        let px = |v: f32| (v * scale) as u32;
        let frames: Vec<Vec<i32>> = (0..60).map(|i| {
            let f = crate::effects::bench::render_frame_at(b.wash, &theme, (W, H), scale, t0 + i as f32 / 60.0, &blocks);
            (px(y0)..px(y1)).flat_map(|y| (px(x0)..px(x1)).map(move |x| (y, x))).map(|(y, x)| luma(f[(y * w + x) as usize]) as i32).collect()
        }).collect();
        let step = 3 * 255 / 4;
        let mut blinks = 0usize;
        for k in 2..frames.len() {
            for p in 0..frames[0].len() {
                let (a, b2, c) = (frames[k - 2][p], frames[k - 1][p], frames[k][p]);
                if ((b2 - a > step) && (b2 - c > step)) || ((a - b2 > step) && (c - b2 > step)) {
                    blinks += 1;
                }
            }
        }
        // Each pixel stands for 1 / scale squared points.
        blinks as f32 / (frames.len() - 2) as f32 / ((x1 - x0) * (y1 - y0)) * 1000.0 / (scale * scale)
    }

    /// How much the sidebar city's pixels jump about from frame to frame beyond moving: the mean
    /// of a pixel's second difference over three frames at 60 a second (a light moving smoothly
    /// through a pixel brightens and dims it gently; one flashing makes it big), out of 765, the
    /// wash drawn at `scale` pixels a point for a second from `t0`.
    fn city_jitter(scale: f32, t0: f32) -> f32 {
        let b = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap();
        let m = crate::effects::manifest::parse(b.manifest).unwrap();
        let theme = crate::theme::builtin("matrix").unwrap();
        let blocks = window(m.gap, 1.0);
        let w = (W as f32 * scale).round() as u32;
        let px = |v: f32| (v * scale) as u32;
        let frames: Vec<Vec<i32>> = (0..60).map(|i| {
            let f = crate::effects::bench::render_frame_at(b.wash, &theme, (W, H), scale, t0 + i as f32 / 60.0, &blocks);
            (px(400.0)..px(630.0)).flat_map(|y| (px(14.0)..px(236.0)).map(move |x| (y, x))).map(|(y, x)| luma(f[(y * w + x) as usize]) as i32).collect()
        }).collect();
        let mut sum = 0u64;
        for k in 2..frames.len() {
            for p in 0..frames[0].len() {
                sum += (frames[k - 2][p] - 2 * frames[k - 1][p] + frames[k][p]).unsigned_abs() as u64;
            }
        }
        sum as f32 / ((frames.len() - 2) * frames[0].len()) as f32
    }

    #[test]
    fn on_a_1x_screen_the_city_s_edges_do_not_flash() {
        // Cyrus, 2026-11-18: "The Matrix City still has the building edges flashing on my external
        // monitor. It's smooth on the native Macbook monitor." The wash is half a pixel a point
        // there; before, an edge moving through its two-point pixels went bright, dim and bright
        // again (0.36 blinks a frame per thousand points, jitter 9.96, against the Retina
        // screen's 0.64 and 10.26).
        if !gpu() {
            return;
        }
        let at = |scale: f32| ((city_blinks(scale, 20.0) + city_blinks(scale, 47.0)) / 2.0, (city_jitter(scale, 20.0) + city_jitter(scale, 47.0)) / 2.0);
        let ((retina_blinks, retina_jitter), (blinks, jitter)) = (at(1.0), at(0.5));
        assert!(blinks < 0.15 && blinks < retina_blinks, "{blinks} blinks a frame per thousand points (Retina {retina_blinks})");
        assert!(jitter < retina_jitter, "jitter {jitter} (Retina {retina_jitter})");
    }

    // The runic world (runic world spec), drawn by the real wash on the GPU.
    fn runic_frame(theme: &crate::theme::Theme, t: f32, blocks: &[crate::effects::blocks::Block]) -> Vec<crate::palette::Rgb> {
        let b = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap();
        crate::effects::bench::render_frame(b.wash, theme, (W, H), t, blocks)
    }

    fn gpu() -> bool {
        metal::Device::system_default().is_some()
    }

    /// The runic effect's edge margin (its `edge_gap`, else its gap).
    fn edge() -> f32 {
        let m = crate::effects::manifest::parse(crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().manifest).unwrap();
        m.edge_gap.unwrap_or(m.gap)
    }

    fn gap() -> f32 {
        crate::effects::manifest::parse(crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().manifest).unwrap().gap
    }

    fn at(frame: &[crate::palette::Rgb], x: f32, y: f32) -> crate::palette::Rgb {
        frame[(y as u32 * W + x as u32) as usize]
    }

    fn luma(c: crate::palette::Rgb) -> u32 {
        c.r as u32 + c.g as u32 + c.b as u32
    }

    /// The brightest pixel of a column of the frame from y0 to y1 at x.
    fn brightest(frame: &[crate::palette::Rgb], x: f32, y0: f32, y1: f32) -> u32 {
        (y0 as u32..y1 as u32).map(|y| luma(at(frame, x, y as f32))).max().unwrap()
    }

    #[test]
    fn on_the_gpu_the_focused_pane_s_frame_is_lit_brighter_than_another_s() {
        if !gpu() {
            return;
        }
        let g = gap();
        let theme = crate::theme::builtin("tokyo-rain").unwrap();
        let frame = runic_frame(&theme, 0.0, &window(g, 0.0));
        // The left pane's left frame line and the right pane's right one, 5 points out, half way
        // down, neither shared with a neighbour: the left pane is focused.
        let (y0, y1) = (300.0, 340.0);
        let lit = brightest(&frame, SIDE + g - 5.0, y0, y1);
        let idle = brightest(&frame, W as f32 - edge() + 5.0, y0, y1);
        assert!(lit > idle + 40, "focused {lit} against {idle}");
    }

    #[test]
    fn on_the_gpu_runes_run_inside_the_focused_pane_s_edges_and_not_inside_another_s() {
        if !gpu() {
            return;
        }
        let g = gap();
        let theme = crate::theme::builtin("tokyo-rain").unwrap();
        // The light in a column on the runes' line, just inside each pane's frame (5 points out
        // from the pane, the line 3.2 in from it: the left pane's left side, the right pane's
        // right side), down its body, summed over a few moments: the left pane is focused.
        let (mut focused, mut other) = (0u32, 0u32);
        for t in [1.0, 3.3, 5.7, 8.2] {
            let frame = runic_frame(&theme, t, &window(g, 0.0));
            for y in 200..500 {
                focused += luma(at(&frame, SIDE + g - 2.0, y as f32));
                other += luma(at(&frame, W as f32 - edge() + 1.0, y as f32));
            }
        }
        assert!(focused > other * 2, "inside the focused pane's frame {focused} against {other}");
    }

    #[test]
    fn on_the_gpu_a_pane_s_strip_reaches_its_frame() {
        if !gpu() {
            return;
        }
        let g = gap();
        let theme = crate::theme::builtin("tokyo-rain").unwrap();
        let frame = runic_frame(&theme, 0.0, &window(g, 0.0));
        // The right pane (not focused, so no runes inside its frame), half way along its bar: 3
        // points inside the frame's line (in the room between the pane and its frame) and 3
        // outside it see the same glow from the line, so the room's pixel is nearer the strip's
        // own (12 points into the bar) than the ground's is.
        let x = SIDE + g + (W as f32 - SIDE - 2.0 * g - edge()) / 2.0 + g + 200.0;
        let top = crate::chrome::TITLE_BAR_H + edge();
        let (room, ground, strip) = (at(&frame, x, top - 2.0), at(&frame, x, top - 8.0), at(&frame, x, top + 12.0));
        let far = |a: crate::palette::Rgb, b: crate::palette::Rgb| (a.r as i32 - b.r as i32).abs() + (a.g as i32 - b.g as i32).abs() + (a.b as i32 - b.b as i32).abs();
        assert!(far(room, strip) < far(ground, strip), "room {room:?} ground {ground:?} strip {strip:?}");
    }

    #[test]
    fn on_the_gpu_the_sidebar_s_frame_lights_while_it_has_the_keys() {
        if !gpu() {
            return;
        }
        let theme = crate::theme::builtin("matrix").unwrap();
        let (off, on) = (runic_frame(&theme, 0.0, &window(gap(), 0.0)), runic_frame(&theme, 0.0, &window(gap(), 1.0)));
        // Its left frame line, 5 points out from its column (which keeps the gap from the
        // window's edge, as a pane does), part way down.
        let x = edge() - 5.0;
        assert!(brightest(&on, x, 200.0, 240.0) > brightest(&off, x, 200.0, 240.0) + 40);
    }

    #[test]
    fn on_the_gpu_the_sidebar_keeps_a_pane_s_margins() {
        if !gpu() {
            return;
        }
        let g = gap();
        let theme = crate::theme::builtin("tokyo-rain").unwrap();
        let frame = runic_frame(&theme, 0.0, &window(g, 0.0));
        // The sidebar's frame line lies as far from the window's left edge as the left pane's
        // lies from the sidebar's column, and the ground between the sidebar's and the pane's
        // frames is as wide as between two panes': its brightest column, half way down, is 5
        // points out from its column, and none brighter lies in the margin before it.
        let y = 320.0;
        let column = |x0: u32, x1: u32| (x0..x1).max_by_key(|&x| luma(at(&frame, x as f32, y))).unwrap();
        // A line's centre on a pixel's edge lights the pixels either side alike: a pixel's slack.
        let near = |found: u32, want: f32| (found as f32 + 0.5 - want).abs() <= 1.0;
        let e = edge();
        let left = column(0, e as u32 + 2);
        assert!(near(left, e - 5.0), "the sidebar's left frame line at {left}");
        let right = column(SIDE as u32 - 2, SIDE as u32 + 8);
        assert!(near(right, SIDE + 5.0), "its right frame line at {right}, 5 out from its column");
    }

    #[test]
    fn on_the_gpu_a_waiting_pane_shows_its_cursor_in_the_waiting_colour() {
        if !gpu() {
            return;
        }
        let theme = crate::theme::builtin("tokyo-rain").unwrap();
        let frame = runic_frame(&theme, 0.0, &window(gap(), 0.0));
        let waiting = theme.semantic.waiting;
        // The right pane waits: its cursor hangs on its frame's foot near its right end.
        let found = (W - 60..W).flat_map(|x| (H - 12..H).map(move |y| (x, y))).any(|(x, y)| at(&frame, x as f32, y as f32) == waiting);
        assert!(found, "no cursor in the waiting colour");
    }

    #[test]
    fn on_the_gpu_the_city_and_the_code_are_two_scenes() {
        if !gpu() {
            return;
        }
        let mut city = crate::theme::builtin("matrix").unwrap();
        city.effect.as_mut().unwrap().scene = Some("city".into());
        let code = crate::theme::builtin("matrix").unwrap();
        let blocks = window(gap(), 0.0);
        let (a, b) = (runic_frame(&city, 0.0, &blocks), runic_frame(&code, 0.0, &blocks));
        // The sidebar's foot, where the scene stands.
        let differ = (20..220).flat_map(|x| (H - 120..H - 20).map(move |y| (x, y))).filter(|&(x, y)| at(&a, x as f32, y as f32) != at(&b, x as f32, y as f32)).count();
        assert!(differ > 2000, "only {differ} points differ");
    }

    /// The sidebar's foot of Tokyo Rain's colours drawn with runic's scene `scene`, at t.
    fn scene_frame(scene: &str, t: f32) -> Vec<crate::palette::Rgb> {
        let mut theme = crate::theme::builtin("tokyo-rain").unwrap();
        theme.effect.as_mut().unwrap().scene = Some(scene.into());
        runic_frame(&theme, t, &window(gap(), 0.0))
    }

    /// How many points of the sidebar's scene band differ between two frames.
    fn band_differ(a: &[crate::palette::Rgb], b: &[crate::palette::Rgb]) -> usize {
        (20..220).flat_map(|x| (H - 220..H - 20).map(move |y| (x, y))).filter(|&(x, y)| at(a, x as f32, y as f32) != at(b, x as f32, y as f32)).count()
    }

    #[test]
    fn on_the_gpu_the_new_cities_are_rainy_skylines_not_the_matrix_street() {
        // The wash drew the Matrix street for any scene over 0.5; Seattle, San Francisco and
        // London (2, 3 and 4) must be the rainy skyline with their own landmarks.
        if !gpu() {
            return;
        }
        let (tokyo, street) = (scene_frame("tokyo", 0.0), scene_frame("matrix-city", 0.0));
        let apart = band_differ(&tokyo, &street);
        for city in ["seattle", "san-francisco", "london"] {
            let d = band_differ(&tokyo, &scene_frame(city, 0.0));
            assert!(d * 2 < apart, "{city} differs from Tokyo in {d} points, as much as the street does ({apart})");
        }
    }

    /// The number after `prefix` in the runic wash's text, up to the next `;` or `,`.
    fn wash_factor(prefix: &str) -> f32 {
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        let at = wash.find(prefix).unwrap_or_else(|| panic!("wash.metal lacks {prefix}")) + prefix.len();
        wash[at..].split([';', ',', ')']).next().unwrap().trim().parse().unwrap()
    }

    #[test]
    fn the_golden_gate_s_two_towers_stand_inside_the_band_at_every_width() {
        // The review of 2026-11-19: the second tower stood past the band's right edge at every
        // sidebar width, so only one of the pair was ever drawn.
        let g1 = wash_factor("g1 = sz.x * ");
        for w in (160..=600).step_by(20) {
            let w = w as f32;
            assert!(g1 * w + 4.9 < w, "the far tower's leg at {} past a {w} point band", g1 * w + 4.9);
        }
    }

    #[test]
    fn salesforce_s_crown_and_the_shard_s_tip_stand_below_the_band_s_faded_top() {
        // The band's top 60 points fade toward the sidebar; a crown from 0.02 of the band's
        // height was mostly faded away, and the Shard's beacon sat inside its own tip.
        assert!(wash_factor("float sx = sz.x * 0.6, stop = sz.y * ") >= 0.15, "Salesforce's top too high");
        assert!(wash_factor("float sx = sz.x * 0.63, stop = sz.y * ") >= 0.06, "the Shard's top too high");
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        assert!(wash.contains("col = beacon_at(q, float2(sx, stop - 1.5), t, u, col);"), "the Shard's beacon above its tip");
    }

    #[test]
    fn seattle_stands_as_seen_from_kerry_park() {
        // Cyrus, 2026-11-19, of Seattle Rain: "balance the heights better with the Columbia
        // Tower", then a photograph of the skyline from Kerry Park: the Needle at the left and
        // tallest, downtown clustered just to its right with the Columbia Center reaching about
        // to the Needle's top house, and Mount Rainier broad and snowy on the right.
        let needle_x = wash_factor("float cx = sz.x * ");
        let needle = wash_factor(&format!("float cx = sz.x * {needle_x}, ttop = sz.y * "));
        let columbia_x = wash_factor("float cd = abs(x - sz.x * ");
        let columbia = wash_factor("float ct = h * ");
        let rainier_x = wash_factor("float px = sz.x * ");
        assert!(needle_x < 0.3, "the Needle at the left ({needle_x})");
        assert!(columbia_x > needle_x && columbia_x < 0.5, "downtown just right of the Needle ({columbia_x})");
        assert!(rainier_x > 0.65, "Rainier on the right ({rainier_x})");
        assert!(columbia > needle && columbia < needle + 0.16, "the Columbia Center's top ({columbia}) about at the Needle's top house, below its spire ({needle})");
        assert!(needle >= 0.08, "below the band's faded top");
    }

    #[test]
    fn downtown_seattle_is_one_silhouette_outlined_along_its_roofs() {
        // Cyrus, 2026-11-19: the towers between the Needle and Rainier "just looks like a bunch of
        // odd lines. Make it look cleaner". Each tower outlined on both sides drew a vertical line
        // wherever two overlapped; downtown is now one shape, outlined only where its outline is.
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        let seattle = &wash[wash.find("float3 seattle_at(").unwrap()..];
        let seattle = &seattle[..seattle.find("\n}\n").unwrap()];
        assert!(wash.contains("float downtown_top(float x, float2 sz)"), "one outline for downtown");
        let downtown = &seattle[..seattle.find("// The Space Needle, in front").unwrap()];
        assert!(!downtown.contains("landmark_at("), "no tower outlined on its own between the Needle and Rainier");
        assert!(downtown.contains("downtown_top(q.x - 1.0, sz)") && downtown.contains("downtown_top(q.x + 1.0, sz)"), "the outline down its outer sides");
    }

    #[test]
    fn rainier_s_foot_is_hidden_behind_the_skylines() {
        // Its base at 0.66 of the band showed a straight cut where a far tower was lower.
        assert_eq!(wash_factor("float px = sz.x * 0.82, py = sz.y * 0.36, base = sz.y * "), 1.0);
    }

    #[test]
    fn the_london_eye_fits_a_narrow_sidebar() {
        // Its radius followed the band's height while every place followed its width, so on a
        // 160 point sidebar it ran over the Elizabeth Tower and the Shard.
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        assert!(wash.contains("float er = min(sz.y * 0.17, sz.x * 0.12);"));
    }

    #[test]
    fn the_matrix_street_looks_south_to_lower_manhattan() {
        // Cyrus, 2026-11-19: "for the Matrix City, I want a cityline in Manhattan looking south at
        // FiDi in the background". A far silhouette at the street's end, drawn before the street's
        // buildings so they cover it: One World Trade Center in the middle, the Woolworth
        // Building, 40 Wall Street, 70 Pine and 8 Spruce Street among plainer towers.
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        let scene = &wash[wash.find("float3 code_scene(").unwrap()..];
        let call = scene.find("col = fidi_at(").expect("code_scene draws Lower Manhattan");
        assert!(call < scene.find("// The horizon's line").unwrap(), "before the horizon's line and the street's buildings");
        let fidi = &wash[wash.find("float3 fidi_at(").expect("a fidi_at function")..];
        let fidi = &fidi[..fidi.find("\n}\n").unwrap()];
        for landmark in ["One World Trade Center", "Woolworth", "40 Wall Street", "70 Pine", "8 Spruce Street"] {
            assert!(fidi.contains(landmark), "fidi_at draws {landmark}");
        }
        assert!(fidi.contains("beacon_at("), "One World Trade Center's beacon");
    }

    #[test]
    fn on_the_gpu_each_rainy_city_draws_its_own_landmarks() {
        if !gpu() {
            return;
        }
        let cities = ["tokyo", "seattle", "san-francisco", "london"];
        let frames: Vec<_> = cities.iter().map(|c| scene_frame(c, 0.0)).collect();
        for i in 0..cities.len() {
            for j in i + 1..cities.len() {
                let d = band_differ(&frames[i], &frames[j]);
                assert!(d > 800, "{} and {} differ in only {d} points", cities[i], cities[j]);
            }
        }
    }

    #[test]
    fn on_the_gpu_the_scene_under_the_names_never_makes_the_dim_text_harder_to_read_than_the_rain() {
        if !gpu() {
            return;
        }
        use crate::palette::contrast;
        // Names running down to 80 points above the window's foot, over most of the scene; at
        // the clock's zero and in the middle of a strike. The rain itself falls behind the
        // sidebar's names, as it always has, so the floor is the 4.5:1 rule or, lower, what the
        // rain alone leaves above the scene: the scene may never take the text below either.
        let names_end = H as f32 - 80.0;
        let worst = |frame: &[crate::palette::Rgb], dim: crate::palette::Rgb, y0: f32, y1: f32| {
            (14..226u32).flat_map(|x| (y0 as u32..y1 as u32).map(move |y| (x, y))).map(|(x, y)| contrast(dim, at(frame, x as f32, y as f32))).fold(f64::MAX, f64::min)
        };
        for name in ["tokyo-rain", "matrix", "seattle", "san-francisco", "london"] {
            // The three rainy cities over Tokyo Rain's colours, as their presets will draw them.
            let mut theme = crate::theme::builtin(if ["tokyo-rain", "matrix"].contains(&name) { name } else { "tokyo-rain" }).unwrap();
            if !["tokyo-rain", "matrix"].contains(&name) {
                theme.effect.as_mut().unwrap().scene = Some(name.into());
            }
            let dim = theme.chrome.text_dim;
            for t in [0.0f32, 14.669] {
                let frame = runic_frame(&theme, t, &window_to(gap(), 0.0, names_end));
                let rain = worst(&frame, dim, crate::chrome::TITLE_BAR_H + 160.0, H as f32 - 260.0);
                let scene = worst(&frame, dim, H as f32 - 230.0, names_end);
                assert!(scene >= rain.min(4.5) - 0.05, "{name} at {t}: the dim text reaches {scene:.2}:1 over the scene, against {rain:.2}:1 over the rain alone");
            }
        }
    }
}
