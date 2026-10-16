//! The config file's `[telemetry]` table (telemetry spec, section 3.1). The app reads it with the
//! rest of its config through `TelemetryConfig`; the daemon reads only this table, with
//! `parse_table`, before every drain and check, so a change applies without a restart.
use serde::{Deserialize, Serialize};
use visits_app::{Crashes, Usage};

/// `usage` is `off`, `local` or `on`, and `crashes` `never`, `ask` or `always`; both have their
/// defaults, so a config without the table is unchanged by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelemetryConfig {
    #[serde(with = "usage")]
    pub usage: Usage,
    #[serde(with = "crashes")]
    pub crashes: Crashes,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self { usage: Usage::Local, crashes: Crashes::Ask }
    }
}

/// Why the table could not be read: the file is not TOML, or the table holds an unknown key or
/// a value that is not one of the choices. The message is toml's first line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "config.toml: {}", self.message)
    }
}

impl std::error::Error for Error {}

/// The `[telemetry]` table of a config file's text, the defaults when it has none. Only this
/// table is read: another section's unknown key or bad value is the app's to report, not this.
pub fn parse_table(config_text: &str) -> Result<TelemetryConfig, Error> {
    let error = |message: String| Error { message: message.lines().next().unwrap_or("invalid TOML").to_string() };
    let mut file: toml::Table = config_text.parse().map_err(|e: toml::de::Error| error(e.message().to_string()))?;
    match file.remove("telemetry") {
        None => Ok(TelemetryConfig::default()),
        Some(table) => TelemetryConfig::deserialize(table).map_err(|e| error(format!("[telemetry]: {}", e.message()))),
    }
}

/// The names a setting's choices are written as, in the order the config's comment lists them,
/// and the choices they name.
const USAGE_NAMES: &[&str] = &["off", "local", "on"];
const USAGE: [Usage; 3] = [Usage::Off, Usage::Local, Usage::On];
const CRASHES_NAMES: &[&str] = &["never", "ask", "always"];
const CRASHES: [Crashes; 3] = [Crashes::Never, Crashes::Ask, Crashes::Always];

/// The choice a string names, or serde's "unknown variant" error listing the names.
fn choice<'de, D: serde::Deserializer<'de>, T: Copy>(d: D, names: &'static [&'static str], choices: &[T]) -> Result<T, D::Error> {
    let s = String::deserialize(d)?;
    names.iter().position(|n| *n == s).map(|i| choices[i]).ok_or_else(|| serde::de::Error::unknown_variant(&s, names))
}

/// The name a choice is written as.
fn name_of<T: PartialEq>(v: &T, names: &[&'static str], choices: &[T]) -> &'static str {
    choices.iter().position(|c| c == v).map_or("", |i| names[i])
}

mod usage {
    use super::*;

    pub fn serialize<S: serde::Serializer>(v: &Usage, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(name_of(v, USAGE_NAMES, &USAGE))
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Usage, D::Error> {
        choice(d, USAGE_NAMES, &USAGE)
    }
}

mod crashes {
    use super::*;

    pub fn serialize<S: serde::Serializer>(v: &Crashes, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(name_of(v, CRASHES_NAMES, &CRASHES))
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Crashes, D::Error> {
        choice(d, CRASHES_NAMES, &CRASHES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config file with every section, as a person's might be.
    const FULL: &str = r#"
[font]
family = "Menlo"
preset = "comfortable"

[font.presets.mine]
size = 17.0
line_height = 1.45

[terminal]
scrollback = 20000
nice = 0

[keybindings]
"split right" = "cmd-d"

[update]
auto = true

[telemetry]
usage = "on"
crashes = "always"
"#;

    #[test]
    fn the_table_of_a_full_config_file_is_read() {
        assert_eq!(parse_table(FULL), Ok(TelemetryConfig { usage: Usage::On, crashes: Crashes::Always }));
        let off = parse_table("[telemetry]\nusage = \"off\"\ncrashes = \"never\"\n").unwrap();
        assert_eq!(off, TelemetryConfig { usage: Usage::Off, crashes: Crashes::Never });
        let local = parse_table("[telemetry]\nusage = \"local\"\ncrashes = \"ask\"\n").unwrap();
        assert_eq!(local, TelemetryConfig::default());
    }

    #[test]
    fn a_config_without_the_table_gives_the_defaults() {
        assert_eq!(TelemetryConfig::default(), TelemetryConfig { usage: Usage::Local, crashes: Crashes::Ask });
        assert_eq!(parse_table(""), Ok(TelemetryConfig::default()));
        assert_eq!(parse_table("[font]\nfamily = \"Menlo\"\n"), Ok(TelemetryConfig::default()));
        assert_eq!(parse_table("[telemetry]\n"), Ok(TelemetryConfig::default()));
        let one = parse_table("[telemetry]\nusage = \"on\"\n").unwrap();
        assert_eq!(one, TelemetryConfig { usage: Usage::On, crashes: Crashes::Ask }, "the other key keeps its default");
    }

    #[test]
    fn an_unknown_key_or_a_bad_value_is_an_error() {
        let e = parse_table("[telemetry]\nusage = \"on\"\nuploads = true\n").unwrap_err();
        assert!(e.message.contains("uploads"), "{e}");
        let e = parse_table("[telemetry]\nusage = \"maybe\"\n").unwrap_err();
        assert!(e.message.contains("maybe"), "{e}");
        let e = parse_table("[telemetry]\ncrashes = true\n").unwrap_err();
        assert!(e.to_string().starts_with("config.toml: "), "{e}");
        assert!(parse_table("telemetry = 1\n").is_err(), "the table must be a table");
        assert!(parse_table("[telemetry\nusage = \"on\"\n").is_err(), "text that is not TOML is an error, never the defaults");
    }

    #[test]
    fn a_broken_other_section_leaves_the_table_readable() {
        let text = "[font]\nfamly = 1\n[terminal]\nscrollback = \"lots\"\n[nonsense]\nx = 1\n[telemetry]\nusage = \"off\"\n";
        assert_eq!(parse_table(text), Ok(TelemetryConfig { usage: Usage::Off, crashes: Crashes::Ask }));
    }

    #[test]
    fn the_table_round_trips_through_toml() {
        for usage in [Usage::Off, Usage::Local, Usage::On] {
            for crashes in [Crashes::Never, Crashes::Ask, Crashes::Always] {
                let c = TelemetryConfig { usage, crashes };
                let text = format!("[telemetry]\n{}", toml::to_string(&c).unwrap());
                assert_eq!(parse_table(&text), Ok(c), "{text}");
            }
        }
        assert_eq!(toml::to_string(&TelemetryConfig::default()).unwrap(), "usage = \"local\"\ncrashes = \"ask\"\n");
    }
}
