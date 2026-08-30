//! Thin wrapper around `SetThreadExecutionState`.
//!
//! Note: this API's state is bound to the **calling thread**, and that thread
//! must stay alive for the request to hold. Every power request in this program
//! therefore has to be issued from the main thread.

use windows_sys::Win32::System::Power::{
    ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, EXECUTION_STATE,
    SetThreadExecutionState,
};

/// Build the flag set for the given switches.
///
/// **`ES_CONTINUOUS` must always be included.** It means "keep this state until
/// told otherwise"; without it the call merely resets the idle timer once, and
/// the machine still sleeps a few minutes later. That failure mode looks like
/// the program is working, which is what makes it dangerous.
fn flags(system: bool, display: bool) -> EXECUTION_STATE {
    let mut f = ES_CONTINUOUS;
    if system {
        f |= ES_SYSTEM_REQUIRED;
    }
    if display {
        f |= ES_DISPLAY_REQUIRED;
    }
    f
}

/// Apply the power request. Returns `false` if the Win32 call failed.
///
/// Must be called from the main (message loop) thread; see the module docs.
pub fn apply(system: bool, display: bool) -> bool {
    // SAFETY: SetThreadExecutionState only reads the bit flags it is handed. It
    // touches no pointers and allocates nothing; it just changes this thread's
    // power request state. The value passed is always a valid flag combination.
    let previous = unsafe { SetThreadExecutionState(flags(system, display)) };
    previous != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_always_include_es_continuous() {
        // The easiest mistake in this project: without ES_CONTINUOUS the API
        // resets the idle timer once instead of holding the state.
        for (sys, disp) in [(false, false), (true, false), (false, true), (true, true)] {
            assert_ne!(
                flags(sys, disp) & ES_CONTINUOUS,
                0,
                "flags({sys}, {disp}) is missing ES_CONTINUOUS"
            );
        }
    }

    #[test]
    fn all_off_is_es_continuous_alone() {
        assert_eq!(flags(false, false), ES_CONTINUOUS);
    }

    #[test]
    fn flags_map_to_the_right_bits() {
        assert_eq!(flags(true, false), ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
        assert_eq!(flags(false, true), ES_CONTINUOUS | ES_DISPLAY_REQUIRED);
        assert_eq!(
            flags(true, true),
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
        );
    }

    #[test]
    fn apply_succeeds_on_a_real_system() {
        // This really does change the calling thread's power state, so it must
        // be restored before the test returns.
        assert!(apply(true, true), "SetThreadExecutionState should not fail");
        assert!(apply(false, false), "restoring should not fail either");
    }
}
