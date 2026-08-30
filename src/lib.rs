//! Code shared by `caffeinate` (the CLI) and `caffeinate-tray` (the tray app).
//!
//! Only genuinely shared pieces live here. The tray's state machine, menu and
//! localisation stay inside that binary, so the CLI does not drag `tray-icon`
//! and `muda` in with it.

pub mod ipc;
pub mod power;
pub mod util;
