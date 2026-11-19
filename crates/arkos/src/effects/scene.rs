//! The scene an effect draws is part of the look: a preset names its own, the custom look
//! names one of its own, and a custom look saved before scenes were chosen keeps the scene its
//! colour's preset gave.
use std::path::Path;

use crate::config::{Config, EffectChoice, Look};
use crate::theme::Theme;

/// The name of the scene the look shows, before the effect's manifest resolves it (an old name,
/// or a name the effect does not list, resolves to its first scene). `theme` is the theme the
/// config shows; `themes_dir` finds the preset a custom colour names.
pub fn look_scene(config: &Config, theme: &Theme, themes_dir: &Path) -> Option<String> {
    let own = |t: &Theme| t.effect.as_ref().and_then(|e| e.scene.clone());
    let named = match (config.look(), config.effect_choice()) {
        (Look::Presets, _) | (_, EffectChoice::Theme) => return own(theme),
        (_, EffectChoice::None) => return None,
        (_, EffectChoice::Named(name)) => name,
    };
    if let Some(scene) = config.scene_choice() {
        return Some(scene.to_string());
    }
    // A custom look saved before scenes were chosen: the scene its colour's preset drew, when
    // that preset draws the same effect.
    let preset = match &config.custom.color {
        Some(color) => crate::theme::load_color(color, themes_dir).ok()?,
        None => theme.clone(),
    };
    preset.effect.as_ref().filter(|e| e.name == named).and(own(&preset))
}

/// The scene to pin as `custom.scene` from what is on screen: the look's scene by its own name,
/// when the effect showing (`effects_dir` finds it) has scenes to choose between, else none.
pub fn shown_scene(config: &Config, theme: &Theme, themes_dir: &Path, effects_dir: &Path) -> Option<String> {
    let name = match config.look() {
        Look::Presets => theme.recommended_effect().map(str::to_string)?,
        Look::Custom => match config.effect_choice() {
            EffectChoice::Named(n) => n,
            EffectChoice::Theme => theme.recommended_effect().map(str::to_string)?,
            EffectChoice::None => return None,
        },
    };
    let m = crate::effects::manifest::load(&name, effects_dir).ok()?.manifest;
    if m.scenes.len() < 2 {
        return None;
    }
    let shown = look_scene(config, theme, themes_dir);
    let own = shown.as_deref().and_then(|s| m.scene_name(s)).unwrap_or(&m.scenes[0]);
    Some(own.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;
    use crate::theme;

    fn dir() -> &'static Path {
        Path::new("/nonexistent-arkos-themes")
    }

    /// The scene number the runic wash is handed for a config.
    fn runic_scene(toml: &str) -> f32 {
        let cfg = parse(toml).unwrap();
        let t = theme::load_look(&cfg, true, dir()).unwrap();
        let m = crate::effects::manifest::load("runic", dir()).unwrap().manifest;
        m.scene_index(look_scene(&cfg, &t, dir()).as_deref())
    }

    #[test]
    fn a_0_8_54_custom_matrix_look_with_runic_keeps_the_matrix_city() {
        assert_eq!(runic_scene("[appearance]\nlook = \"custom\"\n[custom]\ncolor = \"matrix\"\neffect = \"runic\"\n"), 1.0);
    }

    #[test]
    fn a_custom_look_with_runic_over_lavender_shows_tokyo() {
        assert_eq!(runic_scene("[appearance]\nlook = \"custom\"\n[custom]\ncolor = \"lavender\"\neffect = \"runic\"\n"), 0.0);
    }

    #[test]
    fn custom_scene_is_ignored_while_presets_are_in_force() {
        let toml = "[appearance]\nlook = \"presets\"\n[theme]\ndark = \"tokyo-rain\"\n[custom]\ncolor = \"matrix\"\neffect = \"runic\"\nscene = \"matrix-city\"\n";
        let cfg = parse(toml).unwrap();
        assert_eq!(cfg.scene_choice(), None);
        assert_eq!(runic_scene(toml), 0.0);
    }

    #[test]
    fn a_custom_scene_picks_the_scene_whatever_the_colour() {
        let toml = "[appearance]\nlook = \"custom\"\n[custom]\ncolor = \"lavender\"\neffect = \"runic\"\nscene = \"matrix-city\"\n";
        assert_eq!(parse(toml).unwrap().scene_choice(), Some("matrix-city"));
        assert_eq!(runic_scene(toml), 1.0);
        let old_name = "[appearance]\nlook = \"custom\"\n[custom]\ncolor = \"matrix\"\neffect = \"runic\"\nscene = \"city\"\n";
        assert_eq!(runic_scene(old_name), 0.0);
    }

    #[test]
    fn a_stray_scene_on_an_effect_without_scenes_changes_nothing() {
        let cfg = parse("[appearance]\nlook = \"custom\"\n[custom]\ncolor = \"lavender\"\neffect = \"material\"\nscene = \"matrix-city\"\n").unwrap();
        let t = theme::load_look(&cfg, true, dir()).unwrap();
        let m = crate::effects::manifest::load("material", dir()).unwrap().manifest;
        let scene = m.scene_index(look_scene(&cfg, &t, dir()).as_deref());
        assert_eq!(scene, 0.0);
        let mut with = crate::effects::inputs::build(&t, 1.0, (100.0, 100.0), 1.0, 0.0, &[]);
        with.scene = scene;
        let mut without = crate::effects::inputs::build(&t, 1.0, (100.0, 100.0), 1.0, 0.0, &[]);
        without.scene = m.scene_index(None);
        assert_eq!(with.scene, without.scene);
    }
}
