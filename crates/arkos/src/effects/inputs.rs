//! The inputs a frame hands the wash shader, laid out byte for byte as the preamble's
//! `ArkosInputs` (spec 4.2): the clock, the slider, the window, the theme's colours by the
//! preamble's indices, and the blocks.

use super::blocks::Block;
use crate::palette::Rgb;
use crate::theme::Theme;

/// How many blocks the preamble's `ArkosInputs` holds; more are dropped.
pub const MAX_BLOCKS: usize = 48;
/// How many colours the preamble's `ArkosInputs` holds (`ARKOS_COLORS`): the theme's 35, then
/// its four pigments (`ARKOS_PIGMENT`).
pub const COLORS: usize = 39;

/// One block as the preamble's `ArkosBlock`: the rectangle, then kind, edges, seed, value,
/// ordinal and busy as floats. Metal aligns its `float3 pad` to sixteen bytes, so two floats of
/// padding follow the busy flag and the `float3` itself takes a sixteen-byte slot: 64 bytes in all.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ShaderBlock {
    pub rect: [f32; 4],
    pub kind: f32,
    pub edges: f32,
    pub seed: f32,
    pub value: f32,
    pub ordinal: f32,
    /// 1 for a pane's tab bar and body while its active tab's Claude session works, else 0.
    pub busy: f32,
    pub align: [f32; 2],
    pub pad: [f32; 4],
}

/// The preamble's `ArkosInputs`, handed to the shader as buffer 0.
#[repr(C)]
#[derive(Clone, PartialEq)]
pub struct Inputs {
    pub time: f32,
    pub intensity: f32,
    pub size: [f32; 2],
    pub scale: f32,
    pub block_count: f32,
    /// 1 when the theme is dark (light text on a dark ground), 0 when it is light.
    pub dark: f32,
    /// How many of the four pigments are the theme's own (1 to 4); the rest repeat its focus
    /// colour, and an effect makes its own variations for them.
    pub pigment_count: f32,
    pub colors: [[f32; 4]; COLORS],
    pub blocks: [ShaderBlock; MAX_BLOCKS],
}

fn float4(c: Rgb) -> [f32; 4] {
    [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0]
}

/// A colour's hue in degrees, or `None` for a grey (too little colour to have one).
pub fn hue(c: Rgb) -> Option<f32> {
    let (r, g, b) = (c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let chroma = max - min;
    if chroma < 0.08 {
        return None;
    }
    let h = if max == r {
        ((g - b) / chroma).rem_euclid(6.0)
    } else if max == g {
        (b - r) / chroma + 2.0
    } else {
        (r - g) / chroma + 4.0
    };
    Some(h * 60.0)
}

/// The theme's four pigments, the colours an effect tints its panes and highlights with: from its
/// focus colour, its waiting colour, then its red, magenta, blue, cyan, yellow and green, the
/// first four that sit within 90 degrees of hue of its focus colour (a theme's own family: Peach
/// Light's burnt orange, ochre, brick and plum; Lavender Light's purples, rose and blue), the
/// focus colour again for any left over. With them, how many are the theme's own (1 to 4).
pub fn pigments(theme: &Theme) -> ([Rgb; 4], usize) {
    let (c, t, s) = (&theme.chrome, &theme.terminal, &theme.semantic);
    let focus = c.focus_ring;
    let near = |x: Rgb| match (hue(focus), hue(x)) {
        (Some(f), Some(h)) => {
            let d = (f - h).abs();
            d.min(360.0 - d) <= 90.0
        }
        (None, _) => true,
        (Some(_), None) => false,
    };
    let mut out = [focus; 4];
    let mut n = 1;
    for x in [s.waiting, t.red, t.magenta, t.blue, t.cyan, t.yellow, t.green] {
        if n == 4 {
            break;
        }
        if near(x) && !out[..n].contains(&x) {
            out[n] = x;
            n += 1;
        }
    }
    (out, n)
}

/// The theme's colours by the preamble's indices, opaque.
pub fn colors(theme: &Theme) -> [[f32; 4]; COLORS] {
    let (c, t, s) = (&theme.chrome, &theme.terminal, &theme.semantic);
    let (p, _) = pigments(theme);
    [
        c.window_bg,
        c.title_bar,
        c.sidebar,
        c.tab_bar,
        c.tab_active,
        c.tab_inactive,
        c.border,
        c.focus_ring,
        c.text,
        c.text_dim,
        c.banner,
        t.background,
        t.foreground,
        t.selection,
        s.working,
        s.waiting,
        s.bell,
        s.find_match,
        s.find_current,
        t.black,
        t.red,
        t.green,
        t.yellow,
        t.blue,
        t.magenta,
        t.cyan,
        t.white,
        t.bright_black,
        t.bright_red,
        t.bright_green,
        t.bright_yellow,
        t.bright_blue,
        t.bright_magenta,
        t.bright_cyan,
        t.bright_white,
        p[0],
        p[1],
        p[2],
        p[3],
    ]
    .map(float4)
}

/// A frame's inputs: the first `MAX_BLOCKS` of `blocks`, the rest dropped.
pub fn build(theme: &Theme, intensity: f32, size: (f32, f32), scale: f32, time: f32, blocks: &[Block]) -> Inputs {
    let mut out = [ShaderBlock::default(); MAX_BLOCKS];
    let n = blocks.len().min(MAX_BLOCKS);
    for (slot, b) in out.iter_mut().zip(blocks) {
        *slot = ShaderBlock {
            rect: [b.rect.x, b.rect.y, b.rect.w, b.rect.h],
            kind: b.kind as u32 as f32,
            edges: b.edges as f32,
            seed: b.seed,
            value: b.value,
            ordinal: b.ordinal,
            busy: b.busy,
            ..ShaderBlock::default()
        };
    }
    Inputs {
        time,
        intensity,
        size: [size.0, size.1],
        scale,
        block_count: n as f32,
        dark: if theme.is_light() { 0.0 } else { 1.0 },
        pigment_count: pigments(theme).1 as f32,
        colors: colors(theme),
        blocks: out,
    }
}

#[cfg(test)]
mod tests {
    use super::super::blocks::Kind;
    use super::*;
    use crate::tree::Rect;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn the_inputs_struct_is_the_size_the_preamble_declares() {
        // ArkosBlock: float4 rect (16), four floats (16), ordinal and busy (8), then float3 pad
        // aligned to 16: 64 bytes.
        assert_eq!(size_of::<ShaderBlock>(), 64);
        assert_eq!(offset_of!(ShaderBlock, ordinal), 32);
        // The busy flag takes the first float of what was padding; the rest keeps its place.
        assert_eq!(offset_of!(ShaderBlock, busy), 36);
        assert_eq!(offset_of!(ShaderBlock, pad), 48);
        // ArkosInputs: six floats and a float2 (32), float4 colors[39], ArkosBlock blocks[48].
        assert_eq!(COLORS, 39);
        assert_eq!(size_of::<Inputs>(), 32 + 16 * COLORS + 64 * MAX_BLOCKS);
        assert_eq!(size_of::<Inputs>(), 3728);
        assert_eq!(align_of::<Inputs>(), 16);
        assert_eq!(offset_of!(Inputs, block_count), 20);
        assert_eq!(offset_of!(Inputs, colors), 32);
        assert_eq!(offset_of!(Inputs, blocks), 32 + 16 * COLORS);
    }

    #[test]
    fn the_four_pigments_follow_the_ansi_colours_and_are_the_theme_s_own() {
        let peach = crate::theme::builtin("peach-light").unwrap();
        let c = colors(&peach);
        let (p, n) = pigments(&peach);
        assert_eq!(n, 4, "Peach Light has four of its own");
        for k in 0..4 {
            assert_eq!(c[35 + k], float4(p[k]), "ARKOS_PIGMENT + {k}");
        }
        // Peach Light's are its accents as watercolor has always used them: burnt orange, ochre,
        // brick and plum.
        assert_eq!(p, [peach.chrome.focus_ring, peach.semantic.waiting, peach.terminal.red, peach.terminal.magenta]);
        // Lavender Light's sit near its purple: no burnt orange.
        let lavender = crate::theme::builtin("lavender-light").unwrap();
        let (l, _) = pigments(&lavender);
        assert_eq!(l[0], lavender.chrome.focus_ring);
        assert!(!l.contains(&lavender.semantic.waiting), "{l:?}");
        let focus = hue(lavender.chrome.focus_ring).unwrap();
        for c in l {
            let d = (hue(c).unwrap() - focus).abs();
            assert!(d.min(360.0 - d) <= 90.0, "{c:?} is {d} degrees from the focus colour");
        }
        // Every built-in theme gets four.
        for name in ["navy", "timo-purple"] {
            let t = crate::theme::builtin(name).unwrap();
            assert_eq!(pigments(&t).0[0], t.chrome.focus_ring, "{name}");
        }
        // A theme whose accents are all far from its focus colour has a family of one, the focus
        // colour repeated: the shader makes the rest from it.
        let mut lonely = peach.clone();
        lonely.semantic.waiting = Rgb { r: 0, g: 140, b: 60 };
        for c in [&mut lonely.terminal.red, &mut lonely.terminal.magenta, &mut lonely.terminal.yellow] {
            *c = Rgb { r: 30, g: 90, b: 200 };
        }
        lonely.terminal.blue = Rgb { r: 30, g: 90, b: 200 };
        lonely.terminal.cyan = Rgb { r: 0, g: 150, b: 160 };
        lonely.terminal.green = Rgb { r: 0, g: 140, b: 60 };
        assert_eq!(pigments(&lonely), ([lonely.chrome.focus_ring; 4], 1));
        // A grey has no hue and is never a pigment.
        assert_eq!(hue(Rgb { r: 128, g: 128, b: 128 }), None);
    }

    #[test]
    fn the_shader_is_told_whether_the_theme_is_dark_and_how_many_pigments_it_has() {
        let navy = crate::theme::builtin("navy").unwrap();
        let peach = crate::theme::builtin("peach-light").unwrap();
        let at = |t: &Theme| build(t, 1.0, (10.0, 10.0), 1.0, 0.0, &[]);
        assert_eq!((at(&navy).dark, at(&peach).dark), (1.0, 0.0));
        assert_eq!(at(&peach).pigment_count, 4.0);
        assert_eq!(at(&navy).pigment_count, pigments(&navy).1 as f32);
        // In the slot the padding had: the layout is unchanged.
        assert_eq!(offset_of!(Inputs, dark), 24);
        assert_eq!(offset_of!(Inputs, pigment_count), 28);
    }

    #[test]
    fn colours_are_indexed_as_the_preamble_says() {
        let theme = crate::theme::builtin("peach-light").unwrap();
        let c = colors(&theme);
        assert_eq!(c[0], float4(theme.chrome.window_bg));
        assert_eq!(c[8], float4(theme.chrome.text));
        assert_eq!(c[11], float4(theme.terminal.background));
        assert_eq!(c[15], float4(theme.semantic.waiting));
        assert_eq!(c[18], float4(theme.semantic.find_current));
        assert_eq!(c[19], float4(theme.terminal.black));
        assert_eq!(c[20], float4(theme.terminal.red));
        assert_eq!(c[34], float4(theme.terminal.bright_white));
        assert!(c.iter().all(|f| f[3] == 1.0));
    }

    #[test]
    fn more_than_forty_eight_blocks_are_dropped_not_overrun() {
        let theme = crate::theme::builtin("peach-light").unwrap();
        let many: Vec<Block> = (0..60)
            .map(|i| Block { rect: Rect { x: i as f32, y: 0.0, w: 1.0, h: 1.0 }, kind: Kind::PaneBody, edges: 6, seed: i as f32, value: 1.0, ordinal: 0.0, busy: (i % 2) as f32 })
            .collect();
        let inputs = build(&theme, 0.7, (800.0, 600.0), 1.0, 2.5, &many);
        assert_eq!(inputs.block_count, 48.0);
        assert_eq!(inputs.blocks[47].seed, 47.0);
        assert_eq!(inputs.blocks[0].kind, 5.0);
        assert_eq!(inputs.blocks[0].edges, 6.0);
        assert_eq!((inputs.blocks[0].busy, inputs.blocks[1].busy), (0.0, 1.0), "build copies the busy flag");
        assert_eq!((inputs.time, inputs.intensity, inputs.size, inputs.scale), (2.5, 0.7, [800.0, 600.0], 1.0));
        let few = build(&theme, 0.7, (800.0, 600.0), 1.0, 2.5, &many[..3]);
        assert_eq!(few.block_count, 3.0);
        assert_eq!(few.blocks[3], ShaderBlock::default());
    }
}
