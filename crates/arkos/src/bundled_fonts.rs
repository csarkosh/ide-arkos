//! The fonts that ship inside the app. Every font a built-in preset uses is embedded here and
//! registered with gpui's text system at launch, before the first window, so a preset looks the
//! same on every Mac. They are registered for this app only and never installed on the Mac.

use std::borrow::Cow;

/// Every bundled file: the family it is listed under (the name its font file carries) and its bytes.
pub const FILES: &[(&str, &[u8])] = &[
    ("DM Sans", include_bytes!("../fonts/files/dm-sans/DMSans-Bold.ttf")),
    ("DM Sans", include_bytes!("../fonts/files/dm-sans/DMSans-BoldItalic.ttf")),
    ("DM Sans", include_bytes!("../fonts/files/dm-sans/DMSans-Italic.ttf")),
    ("DM Sans", include_bytes!("../fonts/files/dm-sans/DMSans-Light.ttf")),
    ("DM Sans", include_bytes!("../fonts/files/dm-sans/DMSans-LightItalic.ttf")),
    ("DM Sans", include_bytes!("../fonts/files/dm-sans/DMSans-Regular.ttf")),
    ("DM Sans", include_bytes!("../fonts/files/dm-sans/DMSans-SemiBold.ttf")),
    ("DM Sans", include_bytes!("../fonts/files/dm-sans/DMSans-SemiBoldItalic.ttf")),
    ("Roboto Mono", include_bytes!("../fonts/files/roboto-mono/RobotoMono-Bold.ttf")),
    ("Roboto Mono", include_bytes!("../fonts/files/roboto-mono/RobotoMono-BoldItalic.ttf")),
    ("Roboto Mono", include_bytes!("../fonts/files/roboto-mono/RobotoMono-Italic.ttf")),
    ("Roboto Mono", include_bytes!("../fonts/files/roboto-mono/RobotoMono-Regular.ttf")),
    ("Cascadia Code", include_bytes!("../fonts/files/cascadia-code/CascadiaCode-Bold.ttf")),
    ("Cascadia Code", include_bytes!("../fonts/files/cascadia-code/CascadiaCode-BoldItalic.ttf")),
    ("Cascadia Code", include_bytes!("../fonts/files/cascadia-code/CascadiaCode-Italic.ttf")),
    ("Cascadia Code", include_bytes!("../fonts/files/cascadia-code/CascadiaCode-Regular.ttf")),
    ("Monaspace Radon Frozen", include_bytes!("../fonts/files/monaspace-radon/MonaspaceRadonFrozen-Bold.ttf")),
    ("Monaspace Radon Frozen", include_bytes!("../fonts/files/monaspace-radon/MonaspaceRadonFrozen-BoldItalic.ttf")),
    ("Monaspace Radon Frozen", include_bytes!("../fonts/files/monaspace-radon/MonaspaceRadonFrozen-Italic.ttf")),
    ("Monaspace Radon Frozen", include_bytes!("../fonts/files/monaspace-radon/MonaspaceRadonFrozen-Regular.ttf")),
    ("Departure Mono", include_bytes!("../fonts/files/departure-mono/DepartureMono-Regular.otf")),
];

/// The families macOS 14 ships that the built-in font sets name; they are not bundled. Only the
/// tests read it, as the list of what the sets may name beyond the bundle.
#[allow(dead_code)]
pub const MACOS_FAMILIES: [&str; 5] = ["Avenir Next", "Baskerville", "Optima", "PT Mono", "Chalkboard SE"];

/// Registers every bundled file with the text system. A failure is said on stderr and the app
/// goes on with the fonts the Mac has.
pub fn register(cx: &gpui::App) {
    let fonts: Vec<Cow<'static, [u8]>> = FILES.iter().map(|(_, bytes)| Cow::Borrowed(*bytes)).collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("arkos: bundled fonts: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The family name a font file carries: name ID 16 if present, else 1, read from the `name`
    /// table of the sfnt (TrueType and OpenType alike).
    fn family_of(bytes: &[u8]) -> String {
        let u16_at = |o: usize| u16::from_be_bytes([bytes[o], bytes[o + 1]]) as usize;
        let u32_at = |o: usize| u32::from_be_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]) as usize;
        let tables = u16_at(4);
        let name_at = (0..tables)
            .map(|i| 12 + i * 16)
            .find(|&r| &bytes[r..r + 4] == b"name")
            .map(|r| u32_at(r + 8))
            .expect("a name table");
        let count = u16_at(name_at + 2);
        let strings = name_at + u16_at(name_at + 4);
        let mut best: Option<(usize, String)> = None;
        for i in 0..count {
            let r = name_at + 6 + i * 12;
            let (platform, id, len, off) = (u16_at(r), u16_at(r + 6), u16_at(r + 8), u16_at(r + 10));
            if id != 1 && id != 16 {
                continue;
            }
            let raw = &bytes[strings + off..strings + off + len];
            let text = if platform == 0 || platform == 3 {
                String::from_utf16_lossy(&raw.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>())
            } else {
                String::from_utf8_lossy(raw).into_owned()
            };
            if best.as_ref().map_or(true, |(b, _)| id == 16 && *b != 16) {
                best = Some((id, text));
            }
        }
        best.expect("a family name").1
    }

    #[test]
    fn every_family_a_built_in_font_set_names_is_bundled_or_comes_with_macos() {
        for (name, _) in crate::font_sets::BUILTIN {
            let set = crate::font_sets::builtin(name).unwrap();
            for family in [set.prose, set.ui, set.terminal].into_iter().flatten() {
                let known = FILES.iter().any(|(f, _)| *f == family) || MACOS_FAMILIES.contains(&family.as_str());
                assert!(known, "{name} names {family}, which is neither bundled nor a macOS family");
            }
        }
    }

    #[test]
    fn every_bundled_file_is_in_the_family_it_is_listed_under() {
        assert!(!FILES.is_empty());
        for (family, bytes) in FILES {
            assert_eq!(&family_of(bytes), family);
        }
    }

    #[test]
    fn every_bundled_family_has_its_licence_beside_it() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts/files");
        let mut folders = 0;
        for entry in std::fs::read_dir(&root).unwrap() {
            let dir = entry.unwrap().path();
            folders += 1;
            assert!(dir.join("OFL.txt").exists() || dir.join("LICENSE").exists(), "{} has no licence", dir.display());
        }
        assert_eq!(folders, 5);
    }

    #[test]
    fn the_bundled_fonts_add_no_more_than_10_mb() {
        let total: usize = FILES.iter().map(|(_, b)| b.len()).sum();
        assert!(total > 0 && total <= 10 * 1024 * 1024, "{total} bytes");
    }

    #[test]
    fn the_notebook_set_draws_monaspace_radon_frozen() {
        let set = crate::font_sets::builtin("notebook").unwrap();
        assert_eq!(set.terminal.as_deref(), Some("Monaspace Radon Frozen"));
        assert_eq!(set.prose.as_deref(), Some("Chalkboard SE"));
    }
}
