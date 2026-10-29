//! The built-in `matrix` effect's rain, mirrored from its wash shader (`effects/matrix/wash.metal`)
//! so its values can be tested without a GPU. The constants here are the shader's own; a change to
//! one is made to both. Its hash is the shared `effects::hash::hash01`, the shader's own.

/// The width of a column of rain, in points.
pub const RAIN_COL: f32 = 14.0;
/// The height of a row of rain, one rune's cell, in points.
pub const RAIN_ROW: f32 = 18.0;
/// The slowest and fastest a column's streams fall, in rows a second.
pub const RAIN_SPEED_MIN: f32 = 2.0;
pub const RAIN_SPEED_MAX: f32 = 6.0;
/// Seconds a rune shows before its cell picks another.
pub const RUNE_CHANGE: f32 = 0.5;
/// The most the rain adds of its green to the ground, before the user's intensity.
pub const RAIN_ALPHA: f32 = 0.12;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::BUILTIN;

    fn matrix() -> &'static crate::effects::BuiltIn {
        BUILTIN.iter().find(|b| b.name == "matrix").expect("matrix is built in")
    }

    #[test]
    fn the_wash_carries_the_rain_s_constants_and_names_this_mirror() {
        let wash = matrix().wash;
        assert!(wash.contains("effects/matrix.rs"), "the shader names its mirror");
        for (line, value) in [
            ("constant float RAIN_COL = 14.0;", RAIN_COL),
            ("constant float RAIN_ROW = 18.0;", RAIN_ROW),
            ("constant float RAIN_SPEED_MIN = 2.0;", RAIN_SPEED_MIN),
            ("constant float RAIN_SPEED_MAX = 6.0;", RAIN_SPEED_MAX),
            ("constant float RUNE_CHANGE = 0.5;", RUNE_CHANGE),
            ("constant float RAIN_ALPHA = 0.12;", RAIN_ALPHA),
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
            assert!(line.ends_with(&format!(" = {value:?};")), "the mirror's value for {line} is {value:?}");
        }
    }

    #[test]
    fn the_menus_edges_are_drawn_with_the_panes_own_code() {
        let wash = matrix().wash;
        let start = wash.find("fragment float4 arkos_overlay(").expect("matrix draws the menus' edges");
        let overlay = &wash[start..];
        // The nearest edge and its glow come from the functions the panes use.
        for f in ["nearest_edge(", "edge_glow("] {
            assert!(overlay.contains(f), "the menu stage does not call {f}");
            assert!(wash[..start].matches(f).count() >= 2, "{f} is not shared with the wash");
        }
        assert!(overlay.contains("ARKOS_MENU_BLOCK") || overlay.contains("blk.value"), "the edges fade with the menu");
    }
}
