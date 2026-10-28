//! The inputs a frame hands the wash shader, laid out byte for byte as the preamble's
//! `ArkosInputs` (spec 4.2): the clock, the slider, the window, the theme's colours by the
//! preamble's indices, and the blocks.

use super::blocks::Block;
use crate::palette::Rgb;
use crate::theme::Theme;

/// How many blocks the preamble's `ArkosInputs` holds; more are dropped.
pub const MAX_BLOCKS: usize = 48;
/// How many colours the preamble's `ArkosInputs` holds (`ARKOS_COLORS`).
pub const COLORS: usize = 35;

/// One block as the preamble's `ArkosBlock`: the rectangle, then kind, edges, seed, value and
/// ordinal as floats. Metal aligns its `float3 pad` to sixteen bytes, so three floats of padding
/// follow the ordinal and the `float3` itself takes a sixteen-byte slot: 64 bytes in all.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ShaderBlock {
    pub rect: [f32; 4],
    pub kind: f32,
    pub edges: f32,
    pub seed: f32,
    pub value: f32,
    pub ordinal: f32,
    pub align: [f32; 3],
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
    pub pad: [f32; 2],
    pub colors: [[f32; 4]; COLORS],
    pub blocks: [ShaderBlock; MAX_BLOCKS],
}

fn float4(c: Rgb) -> [f32; 4] {
    [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0]
}

/// The theme's colours by the preamble's indices, opaque.
pub fn colors(theme: &Theme) -> [[f32; 4]; COLORS] {
    let (c, t, s) = (&theme.chrome, &theme.terminal, &theme.semantic);
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
            ..ShaderBlock::default()
        };
    }
    Inputs {
        time,
        intensity,
        size: [size.0, size.1],
        scale,
        block_count: n as f32,
        pad: [0.0; 2],
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
        // ArkosBlock: float4 rect (16), four floats (16), ordinal (4), then float3 pad aligned
        // to 16: 64 bytes.
        assert_eq!(size_of::<ShaderBlock>(), 64);
        assert_eq!(offset_of!(ShaderBlock, ordinal), 32);
        assert_eq!(offset_of!(ShaderBlock, pad), 48);
        // ArkosInputs: six floats and a float2 (32), float4 colors[35], ArkosBlock blocks[48].
        assert_eq!(size_of::<Inputs>(), 32 + 16 * COLORS + 64 * MAX_BLOCKS);
        assert_eq!(size_of::<Inputs>(), 3664);
        assert_eq!(align_of::<Inputs>(), 16);
        assert_eq!(offset_of!(Inputs, block_count), 20);
        assert_eq!(offset_of!(Inputs, colors), 32);
        assert_eq!(offset_of!(Inputs, blocks), 32 + 16 * COLORS);
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
            .map(|i| Block { rect: Rect { x: i as f32, y: 0.0, w: 1.0, h: 1.0 }, kind: Kind::PaneBody, edges: 6, seed: i as f32, value: 1.0, ordinal: 0.0 })
            .collect();
        let inputs = build(&theme, 0.7, (800.0, 600.0), 1.0, 2.5, &many);
        assert_eq!(inputs.block_count, 48.0);
        assert_eq!(inputs.blocks[47].seed, 47.0);
        assert_eq!(inputs.blocks[0].kind, 5.0);
        assert_eq!(inputs.blocks[0].edges, 6.0);
        assert_eq!((inputs.time, inputs.intensity, inputs.size, inputs.scale), (2.5, 0.7, [800.0, 600.0], 1.0));
        let few = build(&theme, 0.7, (800.0, 600.0), 1.0, 2.5, &many[..3]);
        assert_eq!(few.block_count, 3.0);
        assert_eq!(few.blocks[3], ShaderBlock::default());
    }
}
