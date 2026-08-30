#![windows_subsystem = "windows"]

mod i18n;
mod power;
mod state;
mod theme;
mod tray;

use std::ptr::null_mut;
use std::time::Instant;

use tray_icon::menu::MenuEvent;
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, KillTimer, MB_ICONERROR, MB_OK, MSG, MessageBoxW,
    PostQuitMessage, SetTimer, TranslateMessage, WM_TIMER,
};

use state::AppState;

const TIMER_INTERVAL_MS: u32 = 1000;

/// A GUID is baked into the name so it cannot collide with another program's
/// named mutex. The `Local\` prefix scopes uniqueness to the current logon
/// session, which is what we want.
const MUTEX_NAME: &str = r"Local\caffeinate-{7C1D4E92-3A6B-4F08-9E5D-2B8A1C0F6D34}";

/// Convert a Rust string into the NUL-terminated UTF-16 buffer Win32 expects.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Is another copy already running?
fn already_running() -> bool {
    let name = wide(MUTEX_NAME);
    // SAFETY: `name` is a valid NUL-terminated UTF-16 buffer that outlives the
    // call. A null first argument means default security attributes. The handle
    // is deliberately never closed: it must live until the process exits, at
    // which point the OS reclaims it.
    unsafe {
        let handle = CreateMutexW(null_mut(), 1, name.as_ptr());
        handle.is_null() || GetLastError() == ERROR_ALREADY_EXISTS
    }
}

/// A fatal startup error. This is the one and only place allowed to pop a dialog.
fn fatal(message: &str) -> ! {
    let body = wide(message);
    let title = wide("caffeinate");
    // SAFETY: both buffers are NUL-terminated and outlive the call; a null hwnd
    // means the message box has no owner window.
    unsafe {
        MessageBoxW(null_mut(), body.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
    std::process::exit(1);
}

fn main() {
    if already_running() {
        // No dialog: double-clicking the executable again should not interrupt
        // the user.
        std::process::exit(0);
    }

    let strings = i18n::detect();

    // Has to happen before any menu exists, or Win32 popups stay light forever.
    theme::follow_system();

    let ui = match tray::Ui::build(strings) {
        Ok(ui) => ui,
        Err(e) => fatal(&format!("{}\n{e}", strings.err_tray)),
    };

    let mut state = AppState::new();
    let mut power_ok = true;
    ui.sync(&state, Instant::now(), power_ok);

    // A timer with a null hwnd posts WM_TIMER straight to this thread's message
    // queue, which also wakes the blocking GetMessageW once a second.
    //
    // Important: with a null hwnd Windows **ignores** the id we pass and picks
    // its own, handing it back as the return value. WM_TIMER carries that id in
    // wParam, so the comparison below must use the returned value. Comparing
    // against a constant of our own makes the condition never true, and the
    // countdown silently stops working.
    // SAFETY: scalar arguments only; a null hwnd is the documented way to
    // create a thread timer.
    let timer_id = unsafe { SetTimer(null_mut(), 0, TIMER_INTERVAL_MS, None) };
    if timer_id == 0 {
        fatal(strings.err_timer);
    }

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

        // SAFETY: `msg` was just filled in by GetMessageW.
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let now = Instant::now();
        let mut power_dirty = false;
        let mut ui_dirty = false;

        // tray-icon's window procedure emits its events synchronously from
        // inside the DispatchMessageW above, so draining the channel right
        // afterwards costs no latency.
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id.0.as_str();

            // Duration entries are `span<index>`, indexing straight into SPANS.
            if let Some(index) = id
                .strip_prefix(tray::ID_SPAN_PREFIX)
                .and_then(|n| n.parse::<usize>().ok())
                && let Some(&span) = state::SPANS.get(index)
            {
                state.set_span(span, now);
                ui_dirty = true;
                continue;
            }

            match id {
                tray::ID_SYSTEM => {
                    state.toggle_system(now);
                    power_dirty = true;
                }
                tray::ID_DISPLAY => {
                    state.toggle_display(now);
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

        if msg.message == WM_TIMER && msg.wParam == timer_id {
            if state.tick(now) {
                // The countdown expired and the state was reset, so the power
                // request has to be released with it.
                power_dirty = true;
            } else if state.remaining(now).is_some() {
                // Mid countdown: only the "remaining" row and tooltip change.
                ui_dirty = true;
            }
        }

        if power_dirty {
            power_ok = power::apply(state.system(), state.display());
        }
        if power_dirty || ui_dirty {
            ui.sync(&state, now, power_ok);
        }
    }

    // Release the power request on the way out. The OS would clear it when the
    // process dies anyway, but being explicit leaves nothing to guess about.
    power::apply(false, false);
    // SAFETY: scalar arguments; calling this on an already dead timer is safe
    // and merely returns FALSE.
    unsafe { KillTimer(null_mut(), timer_id) };
}
