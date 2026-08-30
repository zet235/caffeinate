//! Holds announced by `caffeinate` CLI processes.
//!
//! A lease is display only. The CLI holds its own power request, so nothing
//! here keeps the machine awake; it exists so the tray can show that something
//! is. That also means a stale lease is a cosmetic bug, never a stuck machine.
//!
//! Staleness is handled without a heartbeat: on acquire we open a SYNCHRONIZE
//! handle to the announcing process, and each tick asks whether it has exited.
//! A CLI killed outright therefore drops its lease within a second, and there
//! is no protocol to get wrong.

use caffeinate::ipc::{KIND_ACQUIRE, KIND_RELEASE, Wire};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};

/// `SYNCHRONIZE`. Declared here because windows-sys only exports the constant
/// from a file system module this crate has no other reason to pull in.
const SYNCHRONIZE: u32 = 0x0010_0000;

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
        // stacking up.
        self.release(wire.pid);

        // SAFETY: a scalar call. The returned handle is checked below and owned
        // by the Lease from that point on.
        let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, wire.pid) };
        if handle.is_null() {
            // Without a handle we could never tell when this lease went stale,
            // and a lease that never expires would leave the icon lit forever.
            // Dropping it costs only the status display.
            return false;
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
            // timeout makes this a poll: WAIT_OBJECT_0 means the process object
            // is signalled, which for a process means it has exited.
            let signalled = unsafe { WaitForSingleObject(lease.handle, 0) } == WAIT_OBJECT_0;
            !signalled
        });
        self.list.len() != before
    }

    /// What to show in the menu and tooltip.
    ///
    /// With more than one hold only the newest is named, plus a count. The row
    /// is a status line, not a process list.
    pub fn summary(&self) -> Option<String> {
        let newest = self.list.last()?;
        let extra = self.list.len() - 1;
        if extra == 0 {
            Some(newest.label.clone())
        } else {
            Some(format!("{} (+{extra})", newest.label))
        }
    }
}
