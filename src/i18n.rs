//! Picks the interface strings based on the system display language.
//!
//! **Why the language is not simply a preference:** a Win32 menu takes its font
//! from the system-wide `NONCLIENTMETRICS.lfMenuFont`, and no API can override
//! it for a single `HMENU`. On an English Windows that font is Segoe UI, which
//! has no CJK glyphs, so Chinese text falls through GDI font linking and lands
//! in `MS UI Gothic` (a Japanese face whose embedded bitmap glyphs at small
//! sizes look hard-edged and unantialiased). Only when the system itself is
//! Chinese does `lfMenuFont` become Microsoft JhengHei, where Chinese looks
//! right.
//!
//! So: Chinese system, Chinese UI. Anything else, English.

use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;

use crate::state::SPANS;

/// Every string a user can see.
///
/// The two composed fields are function pointers rather than format strings
/// because the word order differs between languages ("剩餘 01:23:45" versus
/// "01:23:45 remaining"); swapping a prefix would not be enough.
pub struct Strings {
    // Menu entries
    pub keep_system: &'static str,
    pub keep_screen: &'static str,
    pub duration: &'static str,
    /// Labels for the duration entries, aligned by index with [`SPANS`].
    pub spans: [&'static str; SPANS.len()],
    pub no_timer: &'static str,
    pub exit: &'static str,

    // Tooltip fragments describing what is being held
    pub tip_off: &'static str,
    pub tip_system: &'static str,
    pub tip_screen: &'static str,
    pub tip_both: &'static str,
    pub tip_power_failed: &'static str,

    // Startup failures
    pub err_tray: &'static str,
    pub err_timer: &'static str,

    /// Turns `HH:MM:SS` into the menu's "remaining" row.
    pub remaining_text: fn(&str) -> String,
    /// Turns a state description plus an optional `HH:MM:SS` into a tooltip.
    pub tooltip: fn(&str, Option<&str>) -> String,
    /// Labels a hold announced by a `caffeinate` CLI process.
    pub cli_hold: fn(&str) -> String,
}

fn en_remaining(hms: &str) -> String {
    format!("{hms} remaining")
}

fn en_tooltip(what: &str, hms: Option<&str>) -> String {
    match hms {
        Some(hms) => format!("caffeinate: {what}, {hms} remaining"),
        None => format!("caffeinate: {what}"),
    }
}

fn en_cli_hold(label: &str) -> String {
    format!("CLI: {label}")
}

fn zh_remaining(hms: &str) -> String {
    format!("剩餘 {hms}")
}

fn zh_tooltip(what: &str, hms: Option<&str>) -> String {
    match hms {
        Some(hms) => format!("caffeinate：{what} · 剩餘 {hms}"),
        None => format!("caffeinate：{what}"),
    }
}

fn zh_cli_hold(label: &str) -> String {
    format!("CLI：{label}")
}

pub static EN: Strings = Strings {
    keep_system: "Keep system awake",
    keep_screen: "Keep screen on",
    duration: "Duration",
    spans: [
        "Indefinitely",
        "5 minutes",
        "10 minutes",
        "15 minutes",
        "30 minutes",
        "1 hour",
        "2 hours",
        "5 hours",
    ],
    no_timer: "No timer",
    exit: "Exit",

    tip_off: "off",
    tip_system: "system",
    tip_screen: "screen",
    tip_both: "system + screen",
    tip_power_failed: "caffeinate: power request failed",

    err_tray: "Failed to create the tray icon:",
    err_timer: "Failed to create the timer. The countdown will not work.",

    remaining_text: en_remaining,
    tooltip: en_tooltip,
    cli_hold: en_cli_hold,
};

pub static ZH: Strings = Strings {
    keep_system: "電腦不睡眠",
    keep_screen: "螢幕不關閉",
    duration: "持續時間",
    spans: [
        "永久",
        "5 分鐘",
        "10 分鐘",
        "15 分鐘",
        "30 分鐘",
        "1 小時",
        "2 小時",
        "5 小時",
    ],
    no_timer: "未計時",
    exit: "結束",

    tip_off: "未啟用",
    tip_system: "系統",
    tip_screen: "螢幕",
    tip_both: "系統+螢幕",
    tip_power_failed: "caffeinate：電源要求失敗",

    err_tray: "無法建立系統匣圖示：",
    err_timer: "無法建立計時器，倒數功能將無法運作。",

    remaining_text: zh_remaining,
    tooltip: zh_tooltip,
    cli_hold: zh_cli_hold,
};

/// The low 10 bits of a LANGID hold the primary language; `LANG_CHINESE` is
/// 0x04. Simplified and traditional are not distinguished here: any Chinese
/// system gets the traditional strings.
const LANG_CHINESE: u16 = 0x04;
const PRIMARY_LANGID_MASK: u16 = 0x03ff;

/// Pick a string set from the system display language.
pub fn detect() -> &'static Strings {
    // SAFETY: a no-argument call returning a scalar LANGID. It touches no memory.
    let langid = unsafe { GetUserDefaultUILanguage() };
    if langid & PRIMARY_LANGID_MASK == LANG_CHINESE {
        &ZH
    } else {
        &EN
    }
}
