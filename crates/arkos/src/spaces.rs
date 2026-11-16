//! Which Mission Control desktop (Space) a window is on, so a window the daemon relaunches after
//! an update comes back on its own desktop rather than all of them on the one in front (window
//! desktops spec). macOS has no public call for either; SkyLight's private ones answer a process
//! about its own windows: `SLSCopySpacesForWindows`, `SLSCopyManagedDisplaySpaces` and
//! `SLSMoveWindowsToManagedSpace` (checked on macOS 26.6, 2026-11-16: a window moved to a desktop
//! not in front stays there when shown, and the desktop in front does not change). They are
//! looked up at run time, so a macOS without them only loses this. A desktop is kept by its uuid,
//! which lasts across relaunches; its number does not last across a login.
use std::ffi::c_void;
use std::sync::OnceLock;

/// A desktop as SkyLight lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Space {
    pub id: u64,
    pub uuid: String,
    /// An ordinary desktop (type 0), not a full-screen app's own.
    pub user: bool,
}

/// The uuid of the ordinary desktop with number `id`. A display's first desktop has none in
/// SkyLight's list: it is kept as the empty uuid, which finds that one again.
pub fn uuid_of(spaces: &[Space], id: u64) -> Option<String> {
    spaces.iter().find(|s| s.id == id && s.user).map(|s| s.uuid.clone())
}

/// The number, now, of the ordinary desktop with `uuid`: none when it has gone.
pub fn id_of(spaces: &[Space], uuid: &str) -> Option<u64> {
    spaces.iter().find(|s| s.user && s.uuid == uuid).map(|s| s.id)
}

type CFArrayRef = *const c_void;

struct Sky {
    main: unsafe extern "C" fn() -> i32,
    spaces_for_windows: unsafe extern "C" fn(i32, i32, CFArrayRef) -> CFArrayRef,
    managed: unsafe extern "C" fn(i32) -> CFArrayRef,
    move_to: unsafe extern "C" fn(i32, CFArrayRef, u64),
}

/// SkyLight's calls, looked up once.
fn sky() -> Option<&'static Sky> {
    static SKY: OnceLock<Option<Sky>> = OnceLock::new();
    SKY.get_or_init(|| {
        // SAFETY: `dlopen` and `dlsym` take NUL-terminated names; each symbol found is a function
        // of the signature declared for it (SkyLight's, as the window managers call them).
        unsafe {
            let lib = libc::dlopen(c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(), libc::RTLD_LAZY);
            if lib.is_null() {
                return None;
            }
            let sym = |name: &std::ffi::CStr| {
                let p = libc::dlsym(lib, name.as_ptr());
                (!p.is_null()).then_some(p)
            };
            Some(Sky {
                main: std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> i32>(sym(c"SLSMainConnectionID")?),
                spaces_for_windows: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(i32, i32, CFArrayRef) -> CFArrayRef>(sym(c"SLSCopySpacesForWindows")?),
                managed: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(i32) -> CFArrayRef>(sym(c"SLSCopyManagedDisplaySpaces")?),
                move_to: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(i32, CFArrayRef, u64)>(sym(c"SLSMoveWindowsToManagedSpace")?),
            })
        }
    })
    .as_ref()
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

    /// An array SkyLight returned with a +1 count, as Foundation's, released when dropped.
    fn owned(array: CFArrayRef) -> Option<Retained<NSArray<AnyObject>>> {
        // SAFETY: a non-null CFArray is toll-free bridged to NSArray; the copy's count is ours.
        (!array.is_null()).then(|| unsafe { Retained::from_raw(array as *mut NSArray<AnyObject>) }).flatten()
    }

    fn number(v: Option<Retained<AnyObject>>) -> Option<u64> {
        v?.downcast::<NSNumber>().ok().map(|n| n.unsignedLongLongValue())
    }

    /// Every desktop of every display.
    pub fn spaces() -> Vec<Space> {
        let Some(sky) = sky() else { return Vec::new() };
        // SAFETY: the connection is this process's own; the call returns a new array or null.
        let Some(displays) = owned(unsafe { (sky.managed)((sky.main)()) }) else { return Vec::new() };
        let mut out = Vec::new();
        for display in displays.iter() {
            let Ok(display) = display.downcast::<NSDictionary>() else { continue };
            let Some(list) = display.objectForKey(&NSString::from_str("Spaces")).and_then(|l| l.downcast::<NSArray>().ok()) else { continue };
            for space in list.iter() {
                let Ok(space) = space.downcast::<NSDictionary>() else { continue };
                let get = |k: &str| space.objectForKey(&NSString::from_str(k));
                let Some(id) = number(get("ManagedSpaceID")).or_else(|| number(get("id64"))) else { continue };
                let uuid = get("uuid").and_then(|u| u.downcast::<NSString>().ok()).map(|u| u.to_string()).unwrap_or_default();
                out.push(Space { id, uuid, user: number(get("type")) == Some(0) });
            }
        }
        out
    }

    /// The window numbers as a one-element array of numbers.
    fn windows(window: isize) -> Retained<NSArray<NSNumber>> {
        NSArray::from_retained_slice(&[NSNumber::new_isize(window)])
    }

    /// The uuid of the ordinary desktop `window` (an `NSWindow`'s `windowNumber`) is on.
    pub fn space_of(window: isize) -> Option<String> {
        let sky = sky()?;
        let list = windows(window);
        // SAFETY: as above; 7 asks for every kind of space the window is on.
        let ids = owned(unsafe { (sky.spaces_for_windows)((sky.main)(), 7, Retained::as_ptr(&list) as CFArrayRef) })?;
        let id = number(ids.iter().next())?;
        uuid_of(&spaces(), id)
    }

    /// Puts `window` on the ordinary desktop with `uuid`, when there is one now. True
    /// when it was asked to move.
    pub fn move_to(window: isize, uuid: &str) -> bool {
        let Some(sky) = sky() else { return false };
        let Some(id) = id_of(&spaces(), uuid) else { return false };
        let list = windows(window);
        // SAFETY: as above; the array lives across the call.
        unsafe { (sky.move_to)((sky.main)(), Retained::as_ptr(&list) as CFArrayRef, id) };
        true
    }
}

#[cfg(target_os = "macos")]
pub use mac::{move_to, space_of};
#[cfg(all(test, target_os = "macos"))]
use mac::spaces;

#[cfg(test)]
mod tests {
    use super::*;

    fn space(id: u64, uuid: &str, user: bool) -> Space {
        Space { id, uuid: uuid.into(), user }
    }

    #[test]
    fn a_desktop_is_kept_by_its_uuid_and_found_again_by_it_under_its_number_now() {
        let before = [space(5, "", true), space(1816, "A-1", true), space(2561, "B-2", true), space(77, "F-9", false)];
        assert_eq!(uuid_of(&before, 1816).as_deref(), Some("A-1"));
        assert_eq!(uuid_of(&before, 5).as_deref(), Some(""), "the first desktop, which has no uuid");
        assert_eq!(uuid_of(&before, 77), None, "a full-screen app's own space is not one to come back to");
        let after = [space(1, "", true), space(3, "B-2", true), space(9, "A-1", true)];
        assert_eq!(id_of(&after, ""), Some(1));
        assert_eq!(id_of(&after, "A-1"), Some(9), "the same desktop under a new number");
        assert_eq!(id_of(&after, "Z-0"), None, "a desktop that has gone");
    }

    /// Reads this Mac's desktops through SkyLight, as the window will: run by hand with
    /// `--ignored` in a logged-in session (a CI runner may have no window server to ask).
    #[test]
    #[ignore]
    fn this_mac_s_desktops_are_listed_with_their_uuids() {
        let all = spaces();
        eprintln!("{all:?}");
        assert!(all.iter().any(|s| s.user), "at least one ordinary desktop");
        assert_eq!(space_of(-1), None, "no such window");
    }
}
