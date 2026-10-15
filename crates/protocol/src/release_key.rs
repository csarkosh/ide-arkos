//! The public halves of the keys that may sign a release manifest (auto-update spec, section
//! 3). The IDE accepts a manifest signed by any of them; rotating a key is a new entry here and
//! a release signed by both. Base64 of 32 raw ed25519 bytes.
pub const RELEASE_PUBLIC_KEYS: &[&str] = &[
    // Generated 2026-10-14 by `release-tool keygen`; the secret half is the GitHub Actions
    // secret ARKOS_RELEASE_KEY.
    "v21j3+9KeOUsp7rCnlqNh8EKiDKUROz6t2RotcHzvO8=",
];
