//! Effects: shaders drawn under the window and, optionally, over its text (spec 4). An effect is
//! a folder with a manifest and Metal shaders; the built-ins are compiled in, and a folder of
//! the same name in the effects folder replaces one.
// Nothing outside the tests reads these until the wash is drawn from them (a later task).
#![cfg_attr(not(test), allow(dead_code))]

pub mod manifest;

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
pub const BUILTIN: &[BuiltIn] = &[BuiltIn {
    name: "watercolor",
    manifest: include_str!("../../effects/watercolor/effect.toml"),
    wash: include_str!("../../effects/watercolor/wash.metal"),
    glyphs: Some(include_str!("../../effects/watercolor/glyphs.metal")),
}];

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
        assert_eq!(resolve(&EffectChoice::Theme, &peach), Some("watercolor".to_string()));
        assert_eq!(resolve(&EffectChoice::Theme, &navy), None);
        assert_eq!(resolve(&EffectChoice::None, &peach), None);
        assert_eq!(resolve(&EffectChoice::Named("ink".into()), &navy), Some("ink".to_string()));
    }

    #[test]
    fn effects_live_in_the_config_folder_and_the_preamble_declares_the_inputs() {
        assert_eq!(effects_dir(), paths::config_dir().join("effects"));
        assert!(PREAMBLE.contains("struct ArkosInputs"));
        assert!(PREAMBLE.contains("constant int ARKOS_COLORS = 35;"));
    }
}
