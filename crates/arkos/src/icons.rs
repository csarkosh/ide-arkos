//! The app's own icons, served to gpui's `svg()` element by an asset source: drawn in black on a
//! 16 px grid, gpui uses them as masks and fills them with the element's text colour, so an icon
//! takes its colour from the theme at paint time.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

use crate::md_blocks::AlertKind;

/// Serves `ICONS` by path.
pub struct Assets;

const ICONS: &[(&str, &str)] = &[
    (
        "icons/alert-note.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.25" fill="none" stroke="#000" stroke-width="1.5"/><rect x="7.25" y="7" width="1.5" height="4.5" rx="0.75"/><circle cx="8" cy="4.85" r="0.95"/></svg>"##,
    ),
    (
        "icons/alert-tip.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 1.75a4.5 4.5 0 0 0-2.7 8.1c.45.34.7.8.7 1.3v.6h4v-.6c0-.5.25-.96.7-1.3A4.5 4.5 0 0 0 8 1.75z" fill="none" stroke="#000" stroke-width="1.5" stroke-linejoin="round"/><rect x="6" y="13.25" width="4" height="1.5" rx="0.75"/></svg>"##,
    ),
    (
        "icons/alert-important.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M3.25 1.75h9.5a1.5 1.5 0 0 1 1.5 1.5v6.5a1.5 1.5 0 0 1-1.5 1.5H7.5L4.75 14v-2.75h-1.5a1.5 1.5 0 0 1-1.5-1.5v-6.5a1.5 1.5 0 0 1 1.5-1.5z" fill="none" stroke="#000" stroke-width="1.5" stroke-linejoin="round"/><rect x="7.25" y="3.75" width="1.5" height="4" rx="0.75"/><circle cx="8" cy="9.35" r="0.9"/></svg>"##,
    ),
    (
        "icons/alert-warning.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 1.75 14.75 13.75H1.25z" fill="none" stroke="#000" stroke-width="1.5" stroke-linejoin="round"/><rect x="7.25" y="5.5" width="1.5" height="4" rx="0.75"/><circle cx="8" cy="11.4" r="0.9"/></svg>"##,
    ),
    (
        "icons/alert-caution.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M5.1 1.75h5.8l3.35 3.35v5.8l-3.35 3.35H5.1L1.75 10.9V5.1z" fill="none" stroke="#000" stroke-width="1.5" stroke-linejoin="round"/><rect x="7.25" y="4.25" width="1.5" height="4.5" rx="0.75"/><circle cx="8" cy="11" r="0.9"/></svg>"##,
    ),
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS.iter().find(|(p, _)| *p == path).map(|(_, svg)| Cow::Borrowed(svg.as_bytes())))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ICONS.iter().filter(|(p, _)| p.starts_with(path)).map(|(p, _)| SharedString::from(*p)).collect())
    }
}

/// An alert kind's icon path.
pub fn alert(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::Note => "icons/alert-note.svg",
        AlertKind::Tip => "icons/alert-tip.svg",
        AlertKind::Important => "icons/alert-important.svg",
        AlertKind::Warning => "icons/alert-warning.svg",
        AlertKind::Caution => "icons/alert-caution.svg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_alert_has_an_icon_that_parses_as_svg() {
        for kind in [AlertKind::Note, AlertKind::Tip, AlertKind::Important, AlertKind::Warning, AlertKind::Caution] {
            let bytes = Assets.load(alert(kind)).unwrap().expect("served");
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(text.starts_with("<svg") && text.ends_with("</svg>"), "{kind:?}");
        }
        assert!(Assets.load("icons/nope.svg").unwrap().is_none());
        assert_eq!(Assets.list("icons/").unwrap().len(), 5);
    }
}
