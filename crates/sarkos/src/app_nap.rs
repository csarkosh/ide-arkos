//! Opts out of App Nap for the whole process. A minimized or fully occluded window would
//! otherwise have its timers throttled, and the daemon's watchdog reads a late heartbeat as a
//! hung UI (spec 2.4).
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSActivityOptions, NSObjectProtocol, NSProcessInfo, NSString};
use std::sync::OnceLock;

/// The activity token. It is only ever held, never messaged, so sharing it across threads is sound.
struct Token(#[allow(dead_code)] Retained<ProtocolObject<dyn NSObjectProtocol>>);

// SAFETY: the wrapped object is an opaque retained token that is never used after creation; it
// exists only to keep the activity alive, and dropping or holding a retain is thread-safe.
unsafe impl Send for Token {}
unsafe impl Sync for Token {}

static TOKEN: OnceLock<Token> = OnceLock::new();

/// Begins the activity once; later calls do nothing. The token is kept for the process lifetime.
pub fn hold() {
    TOKEN.get_or_init(|| {
        let info = NSProcessInfo::processInfo();
        let reason = NSString::from_str("sarkosd heartbeat");
        // The binding is a safe method: no unsafe block is needed.
        Token(info.beginActivityWithOptions_reason(NSActivityOptions::UserInitiatedAllowingIdleSystemSleep, &reason))
    });
}
