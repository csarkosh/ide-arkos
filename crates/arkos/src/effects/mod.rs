//! Effects: shaders drawn under the window and, optionally, over its text (spec 4). An effect is
//! a folder with a manifest and Metal shaders; the built-ins are compiled in, and a folder of
//! the same name in the effects folder replaces one.
pub mod accent;
pub mod active;
pub mod bench;
pub mod blocks;
pub mod cli;
pub mod decode;
pub mod hash;
pub mod images;
pub mod inputs;
pub mod layer;
pub mod manifest;
pub mod paint;
pub mod scene;
/// The overworld wash's layout and timing mirrored in Rust, for its tests alone: the app never
/// runs it.
#[cfg(test)]
pub mod overworld;
/// The runic wash's rain mirrored in Rust, for its tests alone for now.
#[cfg(test)]
pub mod runic;
/// The space wash's event schedule mirrored in Rust, for its tests alone: the app never runs it.
#[cfg(test)]
pub mod space;
pub mod watercolor;
pub mod stills;
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
    /// Left out of Settings and `arkos effect list`; a config or command that names it still
    /// draws it.
    pub hidden: bool,
    /// The images its manifest names, by the name it gives them.
    pub images: &'static [(&'static str, &'static [u8])],
}

/// Watercolor's gallery (`effects/watercolor/gallery/CREDITS.md`): public-domain works, CC0.
const WATERCOLOR_GALLERY: &[(&str, &[u8])] = &[
    ("gallery/1-great-wave.jpg", include_bytes!("../../effects/watercolor/gallery/1-great-wave.jpg")),
    ("gallery/2-water-lilies.jpg", include_bytes!("../../effects/watercolor/gallery/2-water-lilies.jpg")),
    ("gallery/3-wheat-field-with-cypresses.jpg", include_bytes!("../../effects/watercolor/gallery/3-wheat-field-with-cypresses.jpg")),
    ("gallery/4-sudden-shower.jpg", include_bytes!("../../effects/watercolor/gallery/4-sudden-shower.jpg")),
    ("gallery/5-the-dance-class.jpg", include_bytes!("../../effects/watercolor/gallery/5-the-dance-class.jpg")),
    ("gallery/6-irises.jpg", include_bytes!("../../effects/watercolor/gallery/6-irises.jpg")),
    ("gallery/7-the-harvesters.jpg", include_bytes!("../../effects/watercolor/gallery/7-the-harvesters.jpg")),
    ("gallery/8-water-pitcher.jpg", include_bytes!("../../effects/watercolor/gallery/8-water-pitcher.jpg")),
];

/// The built-in effects, by folder name.
pub const BUILTIN: &[BuiltIn] = &[
    BuiltIn {
        name: "watercolor",
        manifest: include_str!("../../effects/watercolor/effect.toml"),
        wash: include_str!("../../effects/watercolor/wash.metal"),
        glyphs: Some(include_str!("../../effects/watercolor/glyphs.metal")),
        hidden: false,
        images: WATERCOLOR_GALLERY,
    },
    BuiltIn {
        name: "space",
        manifest: include_str!("../../effects/space/effect.toml"),
        wash: include_str!("../../effects/space/wash.metal"),
        glyphs: Some(include_str!("../../effects/space/glyphs.metal")),
        hidden: true,
        images: &[],
    },
    BuiltIn {
        name: "runic",
        manifest: include_str!("../../effects/runic/effect.toml"),
        wash: include_str!("../../effects/runic/wash.metal"),
        glyphs: Some(include_str!("../../effects/runic/glyphs.metal")),
        hidden: false,
        images: &[],
    },
    BuiltIn {
        name: "material",
        manifest: include_str!("../../effects/material/effect.toml"),
        wash: include_str!("../../effects/material/wash.metal"),
        glyphs: None,
        hidden: false,
        images: &[],
    },
    BuiltIn {
        name: "notebook",
        manifest: include_str!("../../effects/notebook/effect.toml"),
        wash: include_str!("../../effects/notebook/wash.metal"),
        glyphs: Some(include_str!("../../effects/notebook/glyphs.metal")),
        hidden: false,
        images: &[],
    },
    BuiltIn {
        name: "overworld",
        manifest: include_str!("../../effects/overworld/effect.toml"),
        wash: include_str!("../../effects/overworld/wash.metal"),
        glyphs: Some(include_str!("../../effects/overworld/glyphs.metal")),
        hidden: false,
        images: &[],
    },
];

/// The name of the effect to draw for the config's choice under `theme`: the theme's own
/// recommendation, nothing, or the name given. It does not check the effect exists; the window
/// does when it installs it.
pub fn resolve(choice: &EffectChoice, theme: &Theme) -> Option<String> {
    match choice {
        EffectChoice::Theme => theme.recommended_effect().map(|n| current_name(n).to_string()),
        EffectChoice::None => None,
        EffectChoice::Named(n) => Some(current_name(n).to_string()),
    }
}

/// Built-in effects that were renamed: (the old name, the name now). A config or a theme file
/// written before the rename still names the old one.
const RENAMED: &[(&str, &str)] = &[("matrix", "runic")];

/// The effect's name now: a built-in's old name gives its new one, any other name itself.
pub fn current_name(name: &str) -> &str {
    RENAMED.iter().find(|(old, _)| *old == name).map_or(name, |(_, new)| new)
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
        let peach = crate::theme::builtin("lavender-watercolor").unwrap();
        let navy = crate::theme::builtin("navy").unwrap();
        let purple = crate::theme::builtin("timo-purple").unwrap();
        assert_eq!(resolve(&EffectChoice::Theme, &peach), Some("watercolor".to_string()));
        assert_eq!(resolve(&EffectChoice::Theme, &navy), Some("material".to_string()), "Material Navy, the default, draws Material");
        let mut bare = navy.clone();
        bare.effect = None;
        assert_eq!(resolve(&EffectChoice::Theme, &bare), None, "a theme that names none draws plain");
        assert_eq!(resolve(&EffectChoice::Theme, &purple), Some("overworld".to_string()));
        assert_eq!(resolve(&EffectChoice::None, &peach), None);
        assert_eq!(resolve(&EffectChoice::Named("ink".into()), &navy), Some("ink".to_string()));
    }

    #[test]
    fn the_rune_rain_is_named_runic_and_its_old_name_matrix_still_draws_it() {
        assert!(BUILTIN.iter().any(|b| b.name == "runic"));
        assert!(!BUILTIN.iter().any(|b| b.name == "matrix"));
        let navy = crate::theme::builtin("navy").unwrap();
        assert_eq!(resolve(&EffectChoice::Named("matrix".into()), &navy), Some("runic".to_string()));
        assert_eq!(resolve(&EffectChoice::Named("space".into()), &navy), Some("space".to_string()));
        // A theme file from before the rename recommends "matrix".
        let old = crate::theme::parse(&crate::theme::builtin_text("matrix").unwrap().replace("name = \"runic\"", "name = \"matrix\"")).unwrap();
        assert_eq!(old.recommended_effect(), Some("matrix"));
        assert_eq!(resolve(&EffectChoice::Theme, &old), Some("runic".to_string()));
        assert_eq!(current_name("matrix"), "runic");
        assert_eq!(current_name("watercolor"), "watercolor");
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
            5 => "Five",
            6 => "Six",
            n => panic!("{n} built-ins: teach this test the word"),
        })));
        assert!(guide.contains("| `glyph_rate` |"), "the manifest table has a glyph_rate row");
        assert!(guide.contains("| `terminal_decode` |"), "the manifest table has a terminal_decode row");
    }
}
