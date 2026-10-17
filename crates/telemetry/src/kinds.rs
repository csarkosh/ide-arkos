//! Arkos's named failure points: the panics Arkos raises on purpose, each with a name a crash
//! report can carry as its `pk` instead of a hash of its message (telemetry spec, section 2.3).
//!
//! A site that wants a name panics with `std::panic::panic_any(Kind::X)`, and the panic hook
//! finds the variant by downcasting the payload. To add one: a variant here with a doc comment
//! saying where it is raised and why, its snake-case name in `Kind::name`, and the variant in
//! `Kind::ALL` (a test checks every name against the wire pattern). The name is sent, so it says
//! what failed in Arkos's terms and never carries a path, a value or anything of the person's.

/// A failure point Arkos names in a crash report.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Not raised anywhere: it keeps the enum and its downcast in place until the first site
    /// that matters gets its own variant.
    Placeholder,
}

impl Kind {
    /// Every variant, so a test can check each name.
    pub const ALL: &'static [Kind] = &[Kind::Placeholder];

    /// The `pk` a report carries for this failure point: the variant's name in snake case.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Placeholder => "placeholder",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_fits_the_wire_pattern_and_is_not_other() {
        for kind in Kind::ALL {
            let name = kind.name();
            let mut chars = name.chars();
            assert!(chars.next().is_some_and(|c| c.is_ascii_lowercase()), "{name}");
            assert!(chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "{name}");
            assert!(name.len() <= 64, "{name}");
            assert_ne!(name, "other", "`other` is the report's word for an unnamed panic");
        }
    }
}
