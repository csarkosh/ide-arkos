//! Opts out of App Nap for the whole process. A minimized or fully occluded window would
//! otherwise have its timers throttled, and the daemon's watchdog reads a late heartbeat as a
//! hung UI (spec 2.4). A second, scoped token keeps the display awake while a video plays.
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
        let reason = NSString::from_str("arkosd heartbeat");
        // The binding is a safe method: no unsafe block is needed.
        Token(info.beginActivityWithOptions_reason(NSActivityOptions::UserInitiatedAllowingIdleSystemSleep, &reason))
    });
}

/// Keeps the display awake while held (`NSActivityIdleDisplaySleepDisabled`, reason "playing
/// video"), what QuickTime Player holds while it plays (spec 3.2). The process wide token above
/// keeps timers running but lets the screen sleep, which a person watching a cut without touching
/// the keyboard must not see. Dropping it ends the activity.
pub struct DisplayAwake(Retained<ProtocolObject<dyn NSObjectProtocol>>);

/// Begins a display activity; it lasts until the returned value is dropped.
pub fn keep_display_awake() -> DisplayAwake {
    let info = NSProcessInfo::processInfo();
    let reason = NSString::from_str("playing video");
    // The binding is a safe method: no unsafe block is needed.
    DisplayAwake(info.beginActivityWithOptions_reason(NSActivityOptions::IdleDisplaySleepDisabled, &reason))
}

impl Drop for DisplayAwake {
    fn drop(&mut self) {
        // SAFETY: the token is the one `beginActivityWithOptions_reason` returned, it is ended
        // exactly once (here, as the value is dropped), and `NSProcessInfo` is thread safe.
        unsafe { NSProcessInfo::processInfo().endActivity(&self.0) };
    }
}
