//! The channel between `caffeinate` (CLI) and `caffeinate-tray`.
//!
//! The CLI always holds its own power request, so the tray is never load
//! bearing: if it is not running, or crashes, or is killed, the CLI is still
//! correct. What travels over this channel is only enough for the tray to
//! *show* that something is holding the machine awake.
//!
//! The transport is `WM_COPYDATA` to a message-only window. It is synchronous,
//! needs no threads on either side (the tray's window procedure runs on its
//! main thread inside `DispatchMessageW`), and is scoped to the current logon
//! session, which matches the single-instance mutex.

use std::cell::RefCell;
use std::ptr::null_mut;

use crate::util::wide;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, FindWindowExW, HWND_MESSAGE, InSendMessage, RegisterClassW,
    SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_COPYDATA, WNDCLASSW,
};

/// Window class of the tray program's message-only window.
const CLASS_NAME: &str = "caffeinate-ipc-{7C1D4E92-3A6B-4F08-9E5D-2B8A1C0F6D34}";

/// Bumped whenever [`Wire`] changes shape. A tray running an older build will
/// see a version it does not recognise and ignore the message rather than
/// misread it.
const WIRE_VERSION: u32 = 2;

pub const KIND_ACQUIRE: u32 = 1;
pub const KIND_RELEASE: u32 = 2;

/// UTF-16 units reserved for the label. Long command lines are truncated; the
/// label is only ever shown in a menu row, so a fixed cap keeps the wire format
/// a plain `#[repr(C)]` struct that can be validated by size alone.
pub const LABEL_CAP: usize = 64;

/// How long to wait for the tray to acknowledge. The CLI announces before it
/// starts the wrapped command, so a tray that has stopped pumping messages must
/// not be able to hold the command hostage.
const SEND_TIMEOUT_MS: u32 = 2000;

/// What crosses the wire. Fixed size on purpose: the receiver can reject
/// anything whose `cbData` does not match exactly, before reading a single
/// field.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Wire {
    pub version: u32,
    pub kind: u32,
    pub pid: u32,
    pub label_len: u32,
    pub label: [u16; LABEL_CAP],
}

impl Wire {
    pub fn new(kind: u32, pid: u32, label: &str) -> Self {
        let mut buf = [0u16; LABEL_CAP];
        let mut len = 0usize;
        // Truncate on a character boundary, not a UTF-16 unit: cutting between
        // the halves of a surrogate pair would leave a lone surrogate that
        // renders as a replacement character.
        for ch in label.chars() {
            if len + ch.len_utf16() > LABEL_CAP {
                break;
            }
            len += ch.encode_utf16(&mut buf[len..]).len();
        }
        Wire {
            version: WIRE_VERSION,
            kind,
            pid,
            label_len: len as u32,
            label: buf,
        }
    }

    /// True if this looks like something we should act on.
    pub fn is_valid(&self) -> bool {
        self.version == WIRE_VERSION
            && (self.kind == KIND_ACQUIRE || self.kind == KIND_RELEASE)
            && self.pid != 0
            && self.label_len as usize <= LABEL_CAP
    }

    /// The label, made safe to drop straight into a menu row.
    ///
    /// The sender is not authenticated, so control characters are folded to
    /// spaces here rather than trusted: a newline or a bidi override in a menu
    /// item is somebody else's problem to have.
    pub fn label_string(&self) -> String {
        let len = (self.label_len as usize).min(LABEL_CAP);
        String::from_utf16_lossy(&self.label[..len])
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    }
}

// ---------------------------------------------------------------- client side

/// A tray program we have found.
///
/// Worth naming rather than passing a bare `HWND` around: comparing two of
/// these is how the CLI notices that the tray it announced to has gone and a
/// different one has taken its place.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Tray(HWND);

/// Locate the running tray, if there is one.
pub fn find_tray() -> Option<Tray> {
    let class = wide(CLASS_NAME);
    // SAFETY: `class` is a valid NUL-terminated UTF-16 buffer alive for the
    // call. Passing HWND_MESSAGE as the parent is the documented way to search
    // message-only windows, which are invisible to plain FindWindowW.
    let hwnd = unsafe { FindWindowExW(HWND_MESSAGE, null_mut(), class.as_ptr(), null_mut()) };
    if hwnd.is_null() {
        None
    } else {
        Some(Tray(hwnd))
    }
}

/// Send one message to a tray we have already located.
pub fn send_to(tray: Tray, msg: &Wire) -> bool {
    let hwnd = tray.0;

    let data = COPYDATASTRUCT {
        dwData: 0,
        cbData: size_of::<Wire>() as u32,
        lpData: msg as *const Wire as *mut _,
    };

    // A plain SendMessageW blocks forever if the receiver has stopped pumping,
    // which would hang the CLI before the wrapped command ever starts. The
    // module promises the tray is never load bearing, and a wedged tray has to
    // count as "not running" for that promise to hold.
    let mut answer: usize = 0;
    // SAFETY: WM_COPYDATA requires the buffer to stay valid for the duration of
    // the call; this one is synchronous, so `msg` and `data` both outlive it.
    // The receiver reads at most `cbData` bytes and copies them out.
    // `answer` is a live local for the out parameter.
    let delivered = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            0 as WPARAM,
            &data as *const COPYDATASTRUCT as LPARAM,
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            &mut answer,
        )
    };

    delivered != 0 && answer != 0
}

/// Find the tray and send one message to it.
///
/// Returns `false` when there is no tray to talk to, which is not an error:
/// the CLI works on its own and simply goes unannounced.
pub fn send(msg: &Wire) -> bool {
    find_tray().is_some_and(|tray| send_to(tray, msg))
}

// ---------------------------------------------------------------- server side

thread_local! {
    /// Messages received by the window procedure, drained by the message loop.
    ///
    /// A thread local is enough because WM_COPYDATA is delivered synchronously
    /// on the thread that owns the window, which is the tray's main thread.
    /// This mirrors how muda hands over menu events.
    static INBOX: RefCell<Vec<Wire>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_COPYDATA {
        // WM_COPYDATA is only meaningful when it was *sent*: lparam then points
        // at the sender's COPYDATASTRUCT, which the system keeps alive for the
        // call. Anyone can POST this message instead, in which case lparam is
        // an arbitrary number and dereferencing it would take the tray down.
        // SAFETY: a no-argument call returning a scalar.
        if unsafe { InSendMessage() } == 0 {
            return 0;
        }

        // SAFETY: for a sent WM_COPYDATA the system guarantees lparam points at a
        // COPYDATASTRUCT valid for the duration of this call. We accept the
        // payload only when its size matches Wire exactly, then copy it out
        // before returning, so nothing outlives the sender's buffer.
        let accepted = unsafe {
            let cds = lparam as *const COPYDATASTRUCT;
            if cds.is_null() {
                false
            } else {
                let cds = &*cds;
                if cds.cbData as usize != size_of::<Wire>() || cds.lpData.is_null() {
                    false
                } else {
                    let wire = std::ptr::read_unaligned(cds.lpData as *const Wire);
                    if wire.is_valid() {
                        INBOX.with(|inbox| inbox.borrow_mut().push(wire));
                        true
                    } else {
                        false
                    }
                }
            }
        };
        return accepted as LRESULT;
    }
    // SAFETY: plain forwarding of a message we do not handle.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// The tray program's receiving end. Dropping it does nothing special; the
/// window lives as long as the process.
pub struct Server {
    _hwnd: HWND,
}

impl Server {
    /// Register the class and create the message-only window.
    ///
    /// Returns `None` if either step fails. That only costs the CLI its status
    /// display, so the tray keeps running either way.
    pub fn start() -> Option<Server> {
        let class = wide(CLASS_NAME);

        // SAFETY: GetModuleHandleW(null) returns this process's own module and
        // cannot fail. The class struct is zeroed first so every field we do
        // not set is null, which is valid for all of them.
        let hwnd = unsafe {
            let hinstance = GetModuleHandleW(null_mut());

            let mut class_def: WNDCLASSW = std::mem::zeroed();
            class_def.lpfnWndProc = Some(wnd_proc);
            class_def.hInstance = hinstance;
            class_def.lpszClassName = class.as_ptr();
            // A duplicate registration returns 0, which is fine: only one
            // instance of the tray runs at a time, guarded by the mutex.
            RegisterClassW(&class_def);

            CreateWindowExW(
                0,
                class.as_ptr(),
                null_mut(),
                0,
                0,
                0,
                0,
                0,
                // HWND_MESSAGE makes this a message-only window: no pixels, no
                // taskbar presence, not enumerated as a top level window.
                HWND_MESSAGE,
                null_mut(),
                hinstance,
                null_mut(),
            )
        };

        if hwnd.is_null() {
            None
        } else {
            Some(Server { _hwnd: hwnd })
        }
    }

    /// Take everything the window procedure has collected since the last call.
    pub fn drain(&self) -> Vec<Wire> {
        INBOX.with(|inbox| std::mem::take(&mut *inbox.borrow_mut()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_label() {
        let w = Wire::new(KIND_ACQUIRE, 42, "cargo build --release");
        assert!(w.is_valid());
        assert_eq!(w.label_string(), "cargo build --release");
        assert_eq!(w.pid, 42);
    }

    #[test]
    fn truncates_an_over_long_label() {
        let long = "x".repeat(LABEL_CAP + 40);
        let w = Wire::new(KIND_ACQUIRE, 1, &long);
        assert_eq!(w.label_len as usize, LABEL_CAP);
        assert_eq!(w.label_string().chars().count(), LABEL_CAP);
        assert!(
            w.is_valid(),
            "truncation must not produce an invalid message"
        );
    }

    #[test]
    fn never_splits_a_surrogate_pair() {
        // Each emoji is two UTF-16 units, so 33 of them straddle the 64 unit
        // cap. Cutting mid pair would leave a lone surrogate, which decodes to
        // a replacement character.
        let long = "\u{1F600}".repeat(33);
        let w = Wire::new(KIND_ACQUIRE, 1, &long);
        assert!(w.label_len as usize <= LABEL_CAP);
        assert_eq!(w.label_string(), "\u{1F600}".repeat(32));
        assert!(
            !w.label_string().contains('\u{FFFD}'),
            "truncation must not manufacture a replacement character"
        );
    }

    #[test]
    fn folds_control_characters_out_of_a_label() {
        // The sender is not authenticated, so a label reaches a menu row only
        // after anything that could break the row is neutralised.
        let w = Wire::new(KIND_ACQUIRE, 1, "build\r\nrm -rf");
        assert_eq!(w.label_string(), "build  rm -rf");
    }

    #[test]
    fn handles_non_ascii_labels() {
        let w = Wire::new(KIND_ACQUIRE, 7, "建置 專案");
        assert_eq!(w.label_string(), "建置 專案");
    }

    #[test]
    fn rejects_a_wrong_version() {
        let mut w = Wire::new(KIND_ACQUIRE, 1, "x");
        w.version = WIRE_VERSION + 1;
        assert!(!w.is_valid());
    }

    #[test]
    fn rejects_an_unknown_kind() {
        let mut w = Wire::new(KIND_ACQUIRE, 1, "x");
        w.kind = 99;
        assert!(!w.is_valid());
    }

    #[test]
    fn rejects_a_zero_pid() {
        // pid 0 is the system idle process and can never own a lease.
        let w = Wire::new(KIND_ACQUIRE, 0, "x");
        assert!(!w.is_valid());
    }

    #[test]
    fn rejects_a_label_length_past_the_buffer() {
        let mut w = Wire::new(KIND_ACQUIRE, 1, "x");
        w.label_len = (LABEL_CAP + 1) as u32;
        assert!(!w.is_valid());
    }
}
