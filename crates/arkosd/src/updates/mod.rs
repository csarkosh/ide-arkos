//! Automatic updates (auto-update spec, sections 5 to 7): the daemon polls a signed manifest,
//! downloads and verifies the release, stages it, and swaps the app bundle when a window asks.
pub mod manifest;
pub mod verify;
pub mod version;

/// The most a release zip may be, whatever the manifest says.
#[allow(dead_code)] // used from Task 3
pub const MAX_ZIP_BYTES: u64 = 200 * 1024 * 1024;
