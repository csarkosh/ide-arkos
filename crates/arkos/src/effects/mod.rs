//! Effects: shaders drawn under the window and, optionally, over its text (spec 4). An effect is
//! a folder with a manifest and Metal shaders; the built-ins are compiled in, and a folder of
//! the same name in the effects folder replaces one.
pub mod active;
pub mod bench;
pub mod blocks;
pub mod cli;
pub mod decode;
pub mod hash;
pub mod inputs;
pub mod layer;
pub mod manifest;
/// The matrix wash's rain mirrored in Rust, for its tests alone for now.
#[cfg(test)]
pub mod matrix;
/// The space wash's event schedule mirrored in Rust, for its tests alone: the app never runs it.
#[cfg(test)]
pub mod space;
pub mod strokes;

use std::path::PathBuf;

use crate::config::EffectChoice;
use crate::paths;
use crate::theme::Theme;

/// The contract every wash shader is compiled with: prepended to its source, and what
/// `arkos effect contract` prints.
pub const PREAMBLE: &str = include_str!("../../effects/arkos.metal");

/// An effect compiled into the binary: its folder's name and the text of each file in it.
pub struct BuiltIn {
    pub name: &'static str,
    pub manifest: &'static str,
    pub wash: &'static str,
    pub glyphs: Option<&'static str>,
}

/// The built-in effects, by folder name.
pub const BUILTIN: &[BuiltIn] = &[
    BuiltIn {
        name: "watercolor",
        manifest: include_str!("../../effects/watercolor/effect.toml"),
        wash: include_str!("../../effects/watercolor/wash.metal"),
        glyphs: Some(include_str!("../../effects/watercolor/glyphs.metal")),
    },
    BuiltIn {
        name: "space",
        manifest: include_str!("../../effects/space/effect.toml"),
        wash: include_str!("../../effects/space/wash.metal"),
        glyphs: Some(include_str!("../../effects/space/glyphs.metal")),
    },
    BuiltIn {
        name: "matrix",
        manifest: include_str!("../../effects/matrix/effect.toml"),
        wash: include_str!("../../effects/matrix/wash.metal"),
        glyphs: Some(include_str!("../../effects/matrix/glyphs.metal")),
    },
];

/// The name of the effect to draw for the config's choice under `theme`: the theme's own
/// recommendation, nothing, or the name given. It does not check the effect exists; the window
/// does when it installs it.
pub fn resolve(choice: &EffectChoice, theme: &Theme) -> Option<String> {
    match choice {
        EffectChoice::Theme => theme.recommended_effect().map(str::to_string),
        EffectChoice::None => None,
        EffectChoice::Named(n) => Some(n.clone()),
    }
}

/// Where a user's effects live, one folder each.
pub fn effects_dir() -> PathBuf {
    paths::config_dir().join("effects")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_follows_the_theme_s_choice_then_none_then_a_name() {
        use crate::config::EffectChoice;
        let peach = crate::theme::builtin("peach-light").unwrap();
        let navy = crate::theme::builtin("navy").unwrap();
        let purple = crate::theme::builtin("timo-purple").unwrap();
        assert_eq!(resolve(&EffectChoice::Theme, &peach), Some("watercolor".to_string()));
        assert_eq!(resolve(&EffectChoice::Theme, &navy), None, "Navy, the default, draws plain");
        assert_eq!(resolve(&EffectChoice::Theme, &purple), None);
        assert_eq!(resolve(&EffectChoice::None, &peach), None);
        assert_eq!(resolve(&EffectChoice::Named("ink".into()), &navy), Some("ink".to_string()));
    }

    #[test]
    fn effects_live_in_the_config_folder_and_the_preamble_declares_the_inputs() {
        assert_eq!(effects_dir(), paths::config_dir().join("effects"));
        assert!(PREAMBLE.contains("struct ArkosInputs"));
        assert!(PREAMBLE.contains("constant int ARKOS_COLORS = 39;"));
        assert!(PREAMBLE.contains("constant int ARKOS_PIGMENT = 35;"));
        assert!(PREAMBLE.contains("float4 colors[39];"));
        assert!(PREAMBLE.contains("float dark;") && PREAMBLE.contains("float pigment_count;"));
    }

    #[test]
    fn the_guide_names_every_built_in_effect() {
        let guide = crate::guide::TEXT;
        let effects = guide.split("## 9. Effects").nth(1).expect("the guide has an Effects section");
        let opening = effects.split("What an effect can touch").next().unwrap();
        for b in BUILTIN {
            let name = format!("`{}`", b.name);
            assert!(opening.contains(&name), "the Effects section's opening names {name}");
        }
        assert!(opening.contains(&format!("{} are built in", match BUILTIN.len() {
            2 => "Two",
            3 => "Three",
            4 => "Four",
            n => panic!("{n} built-ins: teach this test the word"),
        })));
        assert!(guide.contains("| `glyph_rate` |"), "the manifest table has a glyph_rate row");
        assert!(guide.contains("| `terminal_decode` |"), "the manifest table has a terminal_decode row");
    }
}
