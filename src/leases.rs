//! Holds announced by `caffeinate` CLI processes.
//!
//! A lease is display only. The CLI holds its own power request, so nothing
//! here keeps the machine awake; it exists so the tray can show that something
//! is. That also means a stale lease is a cosmetic bug, never a stuck machine.
//!
//! Staleness is handled without a heartbeat: on acquire we open a synchronise
//! handle to the announcing process, and each tick asks whether it has exited.
//! A CLI killed outright therefore drops its lease within a second, and there
//! is no protocol to get wrong.

use caffeinate::ipc::{KIND_ACQUIRE, KIND_RELEASE, Wire};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT};
#[cfg(test)]
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    QueryFullProcessImageNameW, WaitForSingleObject,
};

/// The image an announcing process must be running before its claim is shown.
///
/// The pid crosses the wire unauthenticated, so without this any process in the
/// session could name a long-lived pid and park arbitrary text in the menu with
/// the icon lit for as long as the tray ran.
const CLI_IMAGE: &str = "caffeinate.exe";

/// A cap so a flood of announcements cannot grow the list without bound. The
/// menu names one hold and counts the rest, so nothing above this is even
/// visible.
const MAX_LEASES: usize = 32;

/// The file name of the executable a process is running, lower-cased.
fn image_name(handle: HANDLE) -> Option<String> {
    let mut buf = [0u16; 260];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` and `len` are live locals; `len` is the capacity going in
    // and is overwritten with the length written. 0 selects the Win32 path
    // format. The handle carries PROCESS_QUERY_LIMITED_INFORMATION, which is
    // what this call requires.
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) };
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    // Path::file_name knows both separators on Windows, so this needs no
    // hand-written character list.
    std::path::Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
}

struct Lease {
    pid: u32,
    label: String,
    /// Never null: a lease with no handle is refused rather than stored, since
    /// there would be no way to tell when it went stale.
    handle: HANDLE,
}

impl Drop for Lease {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcess, was checked non-null before
        // the lease was stored, and is closed exactly once, here.
        unsafe { CloseHandle(self.handle) };
    }
}

#[derive(Default)]
pub struct Leases {
    list: Vec<Lease>,
}

impl Leases {
    pub fn new() -> Self {
        Self::default()
    }

    /// Act on one message. Returns `true` if the set of leases changed.
    pub fn apply(&mut self, wire: &Wire) -> bool {
        match wire.kind {
            KIND_ACQUIRE => self.acquire(wire),
            KIND_RELEASE => self.release(wire.pid),
            _ => false,
        }
    }

    fn acquire(&mut self, wire: &Wire) -> bool {
        // A process announcing twice replaces its own entry rather than
        // stacking up. Whether that removal happened matters even if the rest
        // of this fails: the caller only refreshes the UI when told something
        // changed, so losing this would leave a stale row on screen forever.
        let replaced = self.release(wire.pid);

        if self.list.len() >= MAX_LEASES {
            return replaced;
        }

        // SAFETY: a scalar call. The returned handle is checked below and owned
        // by the Lease from that point on.
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                wire.pid,
            )
        };
        if handle.is_null() {
            // Without a handle we could never tell when this lease went stale,
            // and a lease that never expires would leave the icon lit forever.
            // Dropping it costs only the status display.
            return replaced;
        }

        // Nothing about the message proves the sender is who it says, so check
        // the pid really is running the CLI before showing anything it claims.
        if image_name(handle).as_deref() != Some(CLI_IMAGE) {
            // SAFETY: opened just above, not yet owned by a Lease, closed once.
            unsafe { CloseHandle(handle) };
            return replaced;
        }

        self.list.push(Lease {
            pid: wire.pid,
            label: wire.label_string(),
            handle,
        });
        true
    }

    fn release(&mut self, pid: u32) -> bool {
        let before = self.list.len();
        self.list.retain(|lease| lease.pid != pid);
        self.list.len() != before
    }

    /// Drop leases whose process has exited. Returns `true` if any went away.
    pub fn prune(&mut self) -> bool {
        let before = self.list.len();
        self.list.retain(|lease| {
            // SAFETY: the handle is valid for the lease's lifetime. A zero
            // timeout makes this a poll.
            //
            // WAIT_TIMEOUT is the only answer that means "still running".
            // WAIT_OBJECT_0 means the process object is signalled, which for a
            // process means it exited; anything else, WAIT_FAILED included,
            // means we can no longer tell, and a lease we cannot verify must
            // not be the one thing that outlives its process.
            let waited = unsafe { WaitForSingleObject(lease.handle, 0) };
            waited == WAIT_TIMEOUT
        });
        self.list.len() != before
    }

    /// What to show in the menu and tooltip.
    ///
    /// With more than one hold only the newest is named, plus a count. The row
    /// is a status line, not a process list.
    pub fn summary(&self) -> Option<String> {
        summarise(self.list.iter().map(|lease| lease.label.as_str()))
    }
}

/// The status line for a set of hold labels, oldest first.
///
/// Split out from [`Leases`] so the one piece of real judgement in this module
/// can be tested: everything else here needs a live process to mean anything.
fn summarise<'a>(labels: impl DoubleEndedIterator<Item = &'a str>) -> Option<String> {
    let labels: Vec<&str> = labels.collect();
    let newest = labels.last()?;
    let extra = labels.len() - 1;
    if extra == 0 {
        Some((*newest).to_string())
    } else {
        Some(format!("{newest} (+{extra})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_image_name_of_a_live_process() {
        // The whole sender check rests on this call working, and it cannot be
        // exercised without a real process handle. The test binary is one.
        // SAFETY: a no-argument call returning a pseudo handle for this
        // process. It carries full access and must not be closed.
        let me = unsafe { GetCurrentProcess() };
        let name = image_name(me).expect("a live process has an image name");
        assert!(
            name.ends_with(".exe"),
            "expected an executable name, got {name}"
        );
        assert_eq!(
            name,
            name.to_ascii_lowercase(),
            "must be lower-cased to compare"
        );
        assert!(
            !name.chars().any(std::path::is_separator),
            "must be a file name, not a path: {name}"
        );
    }

    #[test]
    fn nothing_held_says_nothing() {
        assert_eq!(summarise([].into_iter()), None);
    }

    #[test]
    fn a_single_hold_is_named_on_its_own() {
        assert_eq!(
            summarise(["cargo build"].into_iter()),
            Some("cargo build".to_string())
        );
    }

    #[test]
    fn extra_holds_are_counted_behind_the_newest() {
        // Newest last: the most recent announcement is the one worth naming.
        assert_eq!(
            summarise(["old", "newer", "newest"].into_iter()),
            Some("newest (+2)".to_string())
        );
    }
}
