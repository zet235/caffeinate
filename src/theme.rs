//! Makes Win32 menus follow the system light/dark setting.
//!
//! muda builds menus with the standard `CreatePopupMenu` + `TrackPopupMenu`,
//! and Win32 popup menus **always render light** unless the process opts in
//! with uxtheme.dll. The awkward part is that these functions are undocumented
//! and exported by ordinal only, with no names in the DLL. They have existed
//! since Windows 10 1809.
//!
//! If the lookup fails we give up quietly. The worst outcome is a light menu,
//! which is not worth aborting over.

use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

/// Undocumented uxtheme.dll ordinals.
const ORDINAL_SET_PREFERRED_APP_MODE: usize = 135;
const ORDINAL_FLUSH_MENU_THEMES: usize = 136;

/// Argument to `SetPreferredAppMode`. Only `AllowDark` is used here: it means
/// "follow the system setting", not "force dark".
#[repr(i32)]
#[derive(Clone, Copy)]
enum PreferredAppMode {
    AllowDark = 1,
}

/// The return value is the *previous* mode, which is 0 (Default) on the first
/// call of a process. It is deliberately typed as a plain `i32` and not as
/// `PreferredAppMode`: that enum has exactly one valid discriminant, so
/// materialising a 0 as one would be instant undefined behaviour even though
/// the value is thrown away.
type SetPreferredAppMode = unsafe extern "system" fn(PreferredAppMode) -> i32;
type FlushMenuThemes = unsafe extern "system" fn();

/// Opt into the system theme. **Must run before any menu is created.**
pub fn follow_system() {
    // SAFETY: both functions are resolved by ordinal and skipped if absent, so
    // no null pointer is ever called. Their signatures come from the
    // win32-darkmode reverse engineering work and match the aliases above.
    // Both only mutate process-level theme state and touch none of our memory.
    unsafe {
        let uxtheme = LoadLibraryA(c"uxtheme.dll".as_ptr() as *const u8);
        if uxtheme.is_null() {
            return;
        }

        if let Some(addr) = GetProcAddress(uxtheme, ORDINAL_SET_PREFERRED_APP_MODE as *const u8) {
            let set_preferred_app_mode: SetPreferredAppMode = std::mem::transmute(addr);
            set_preferred_app_mode(PreferredAppMode::AllowDark);
        }

        // The opt-in has to be followed by a flush, or already cached menu
        // themes will not pick it up.
        if let Some(addr) = GetProcAddress(uxtheme, ORDINAL_FLUSH_MENU_THEMES as *const u8) {
            let flush_menu_themes: FlushMenuThemes = std::mem::transmute(addr);
            flush_menu_themes();
        }
    }
}
