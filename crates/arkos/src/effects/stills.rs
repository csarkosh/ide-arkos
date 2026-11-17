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
        let top = crate::chrome::TITLE_BAR_H;
        let tab = crate::pane_view::TAB_BAR_H;
        let content = Rect { x: SIDE + gap, y: top + gap, w: W as f32 - SIDE - 2.0 * gap, h: H as f32 - top - 2.0 * gap };
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
        let scene = Scene { gap, focus: &focus, sidebar_focus, waiting: &waiting, ..crate::effects::active::scene(content, 0.0, Some(top + 330.0), Some(top + 300.0), &panes, &busy, &shapes, &strokes) };
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
        for (effect, theme) in [("runic", "tokyo-rain"), ("runic", "matrix"), ("overworld", "timo-purple")] {
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
}
