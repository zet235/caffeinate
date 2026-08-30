#![windows_subsystem = "windows"]

mod i18n;
mod leases;
mod state;
mod theme;
mod tray;

use std::cell::RefCell;
use std::ptr::null_mut;
use std::time::Instant;

use caffeinate::util::wide;
use caffeinate::{ipc, power};
use tray_icon::menu::MenuEvent;
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, KillTimer, MB_ICONERROR, MB_OK, MSG, MessageBoxW,
    PostQuitMessage, SetTimer, TranslateMessage,
};

use leases::Leases;
use state::AppState;
use tray::Ui;

const TIMER_INTERVAL_MS: u32 = 1000;

/// A GUID is baked into the name so it cannot collide with another program's
/// named mutex. The `Local\` prefix scopes uniqueness to the current logon
/// session, which is what we want.
const MUTEX_NAME: &str = r"Local\caffeinate-{7C1D4E92-3A6B-4F08-9E5D-2B8A1C0F6D34}";

/// Outcome of the single-instance check.
///
/// "Could not tell" is deliberately not folded into "already running": exiting
/// silently on a real failure would look exactly like a successful launch that
/// did nothing, with no icon and no message to explain it.
enum Instance {
    First,
    AlreadyRunning,
    CheckFailed,
}

fn check_instance() -> Instance {
    let name = wide(MUTEX_NAME);
    // SAFETY: `name` is a valid NUL-terminated UTF-16 buffer that outlives the
    // call. A null first argument means default security attributes. The handle
    // is deliberately never closed: it must live until the process exits, at
    // which point the OS reclaims it. GetLastError is read immediately after,
    // with nothing in between that could overwrite it.
    unsafe {
        let handle = CreateMutexW(null_mut(), 1, name.as_ptr());
        if handle.is_null() {
            Instance::CheckFailed
        } else if GetLastError() == ERROR_ALREADY_EXISTS {
            Instance::AlreadyRunning
        } else {
            Instance::First
        }
    }
}

/// A fatal startup error. This is the one and only place allowed to pop a dialog.
fn fatal(message: &str) -> ! {
    let body = wide(message);
    let title = wide("caffeinate");
    // SAFETY: both buffers are NUL-terminated and outlive the call; a null hwnd
    // means the message box has no owner window.
    unsafe {
        MessageBoxW(
            null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
    std::process::exit(1);
}

/// Everything the message loop and the timer callback both work on.
///
/// It lives in a thread local because a `TIMERPROC` is a bare function with
/// nowhere to hang state. That costs nothing in practice: `Ui` is full of `Rc`s
/// and the power request is bound to this thread, so none of it could move to
/// another thread anyway.
struct App {
    state: AppState,
    leases: Leases,
    ui: Ui,
    ipc: Option<ipc::Server>,
    power_ok: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

impl App {
    /// Take in whatever has arrived and push the result to the UI.
    ///
    /// `tick` is true only on the once-a-second pass, which is what advances
    /// the countdown and expires dead leases.
    fn pump(&mut self, now: Instant, tick: bool) {
        let mut power_dirty = false;
        let mut ui_dirty = false;

        // WM_COPYDATA is a *sent* message: its window procedure runs inside
        // GetMessageW, which does not return for sent messages. So these are
        // collected on a later pass rather than being "already waiting" the
        // moment a DispatchMessageW returns.
        if let Some(server) = &self.ipc {
            for wire in server.drain() {
                if self.leases.apply(&wire) {
                    ui_dirty = true;
                }
            }
        }

        // tray-icon's window procedure emits its events synchronously from
        // inside DispatchMessageW, so draining the channel costs no latency.
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id.0.as_str();

            // Duration entries are `span<index>`, indexing straight into SPANS.
            if let Some(index) = id
                .strip_prefix(tray::ID_SPAN_PREFIX)
                .and_then(|n| n.parse::<usize>().ok())
                && let Some(&span) = state::SPANS.get(index)
            {
                self.state.set_span(span, now);
                ui_dirty = true;
                continue;
            }

            match id {
                tray::ID_SYSTEM => {
                    self.state.toggle_system(now);
                    power_dirty = true;
                }
                tray::ID_DISPLAY => {
                    self.state.toggle_display(now);
                    power_dirty = true;
                }
                tray::ID_QUIT => {
                    // SAFETY: a scalar call with no arguments; the next
                    // GetMessageW picks up WM_QUIT and leaves the loop.
                    unsafe { PostQuitMessage(0) };
                }
                _ => {}
            }
        }

        if tick {
            // A CLI that was killed never sends its release, so every tick
            // checks whether the announcing processes are still alive.
            if self.leases.prune() {
                ui_dirty = true;
            }
            if self.state.tick(now) {
                // The countdown expired and the state was reset, so the power
                // request has to be released with it.
                power_dirty = true;
            } else if self.state.remaining(now).is_some() {
                // Mid countdown: only the "remaining" row and tooltip change.
                ui_dirty = true;
            }
        }

        if power_dirty {
            self.power_ok = power::apply(self.state.system(), self.state.display());
        }
        if power_dirty || ui_dirty {
            let hold = self.leases.summary();
            self.ui
                .sync(&self.state, now, self.power_ok, hold.as_deref());
        }
    }
}

/// Run one pass over the shared state, unless one is already running.
///
/// `try_borrow_mut` rather than `borrow_mut`: `Shell_NotifyIconW` inside
/// `Ui::sync` is a cross-process send, and Windows pumps this thread's queue
/// while it blocks, so the timer callback can land in the middle of a pass
/// already under way. Skipping is right there: the pass in progress is about to
/// publish a newer state anyway, and the next tick is only a second off.
fn pump(now: Instant, tick: bool) {
    APP.with(|cell| {
        if let Ok(mut app) = cell.try_borrow_mut()
            && let Some(app) = app.as_mut()
        {
            app.pump(now, tick);
        }
    });
}

/// The once-a-second tick.
///
/// This has to be a `TIMERPROC` rather than a `WM_TIMER` the main loop reads.
/// While a popup menu is open Win32 runs its own modal message loop, and that
/// loop drains the queue: a bare `WM_TIMER` posted by a null-hwnd `SetTimer`
/// would be dispatched there and never reach our `GetMessageW`, so the
/// countdown would silently stop for as long as the menu stayed open. A
/// `TIMERPROC` is invoked *by* `DispatchMessageW`, so the modal loop calls it
/// too and the clock keeps running.
///
/// Nothing in here may panic: unwinding out of an `extern "system"` function
/// aborts the process.
unsafe extern "system" fn on_timer(_hwnd: HWND, _msg: u32, _id: usize, _ticks: u32) {
    pump(Instant::now(), true);
}

fn main() {
    let strings = i18n::detect();

    match check_instance() {
        Instance::First => {}
        // No dialog: double-clicking the executable again should not interrupt
        // the user.
        Instance::AlreadyRunning => std::process::exit(0),
        Instance::CheckFailed => fatal(strings.err_mutex),
    }

    // Has to happen before any menu exists, or Win32 popups stay light forever.
    theme::follow_system();

    // The timer goes up before the tray icon so that failing here has no icon
    // to strand: `fatal` exits without running destructors, and `TrayIcon`'s is
    // what takes the icon back out of the notification area.
    //
    // With a null hwnd Windows ignores the id we pass and assigns its own,
    // returning it. That returned value is what `KillTimer` needs below.
    // SAFETY: scalar arguments plus a function pointer matching TIMERPROC; a
    // null hwnd is the documented way to create a thread timer.
    let timer_id = unsafe { SetTimer(null_mut(), 0, TIMER_INTERVAL_MS, Some(on_timer)) };
    if timer_id == 0 {
        fatal(strings.err_timer);
    }

    let ui = match Ui::build(strings) {
        Ok(ui) => ui,
        Err(e) => fatal(&format!("{}\n{e}", strings.err_tray)),
    };

    // The CLI announces its holds here. Failing to open the channel only costs
    // the status display, so the tray carries on without it.
    let app = App {
        state: AppState::new(),
        leases: Leases::new(),
        ui,
        ipc: ipc::Server::start(),
        power_ok: true,
    };
    app.ui.sync(&app.state, Instant::now(), app.power_ok, None);
    APP.with(|cell| *cell.borrow_mut() = Some(app));

    // SAFETY: MSG is plain old data, so an all-zero value is valid; GetMessageW
    // overwrites it before it is read.
    let mut msg: MSG = unsafe { std::mem::zeroed() };

    loop {
        // GetMessageW: >0 for a normal message, 0 for WM_QUIT, -1 on error.
        // SAFETY: `msg` is valid writable memory owned by this function; a null
        // hwnd means "any message for this thread".
        let result = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
        if result <= 0 {
            break;
        }

        // SAFETY: `msg` was just filled in by GetMessageW. Dispatching is what
        // runs tray-icon's window procedure and `on_timer`.
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        pump(Instant::now(), false);
    }

    // Release the power request on the way out. The OS would clear it when the
    // process dies anyway, but being explicit leaves nothing to guess about.
    power::apply(false, false);
    // SAFETY: scalar arguments; calling this on an already dead timer is safe
    // and merely returns FALSE.
    unsafe { KillTimer(null_mut(), timer_id) };
    // Drop the tray icon now, so it leaves the notification area immediately
    // rather than when the shell next notices the process is gone.
    APP.with(|cell| cell.borrow_mut().take());
}
