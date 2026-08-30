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
/// The composed fields are function pointers rather than format strings because
/// word order differs between languages ("剩餘 01:23:45" versus "01:23:45
/// remaining"); swapping a prefix would not be enough. Composition lives here
/// too, so no caller ever glues visible text together itself.
pub struct Strings {
    // Menu entries
    pub keep_system: &'static str,
    pub keep_screen: &'static str,
    pub duration: &'static str,
    /// Labels for the duration entries, aligned by index with [`SPANS`].
    pub spans: [&'static str; SPANS.len()],
    pub no_timer: &'static str,
    pub exit: &'static str,

    // Tooltip fragments describing what the user's own switches hold
    pub tip_system: &'static str,
    pub tip_screen: &'static str,
    pub tip_both: &'static str,
    pub tip_power_failed: &'static str,

    // Startup failures
    pub err_tray: &'static str,
    pub err_timer: &'static str,
    pub err_mutex: &'static str,

    /// Turns `HH:MM:SS` into the menu's "remaining" row.
    pub remaining_text: fn(&str) -> String,
    /// Labels a hold announced by a `caffeinate` CLI process.
    pub cli_hold: fn(&str) -> String,
    /// Builds the whole tooltip.
    ///
    /// `what` is what the user's own switches hold, `None` when both are off;
    /// `hms` is the countdown; `hold` is a CLI hold. All three absent means
    /// idle. This function owns every separator, which is also why a CLI-only
    /// hold can never render as "off" beside a lit icon.
    pub tooltip: fn(Option<&str>, Option<&str>, Option<&str>) -> String,
}

fn en_remaining(hms: &str) -> String {
    format!("{hms} remaining")
}

fn en_cli_hold(label: &str) -> String {
    format!("CLI: {label}")
}

fn en_tooltip(what: Option<&str>, hms: Option<&str>, hold: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(what) = what {
        parts.push(match hms {
            Some(hms) => format!("{what}, {hms} remaining"),
            None => what.to_string(),
        });
    }
    if let Some(hold) = hold {
        parts.push(en_cli_hold(hold));
    }
    if parts.is_empty() {
        parts.push("off".to_string());
    }
    format!("caffeinate: {}", parts.join(" · "))
}

fn zh_remaining(hms: &str) -> String {
    format!("剩餘 {hms}")
}

fn zh_cli_hold(label: &str) -> String {
    format!("CLI：{label}")
}

fn zh_tooltip(what: Option<&str>, hms: Option<&str>, hold: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(what) = what {
        parts.push(match hms {
            Some(hms) => format!("{what} · 剩餘 {hms}"),
            None => what.to_string(),
        });
    }
    if let Some(hold) = hold {
        parts.push(zh_cli_hold(hold));
    }
    if parts.is_empty() {
        parts.push("未啟用".to_string());
    }
    format!("caffeinate：{}", parts.join(" · "))
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

    tip_system: "system",
    tip_screen: "screen",
    tip_both: "system + screen",
    tip_power_failed: "caffeinate: power request failed",

    err_tray: "Failed to create the tray icon:",
    err_timer: "Failed to create the timer. The countdown will not work.",
    err_mutex: "Failed to check whether another copy is already running.",

    remaining_text: en_remaining,
    cli_hold: en_cli_hold,
    tooltip: en_tooltip,
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

    tip_system: "系統",
    tip_screen: "螢幕",
    tip_both: "系統+螢幕",
    tip_power_failed: "caffeinate：電源要求失敗",

    err_tray: "無法建立系統匣圖示：",
    err_timer: "無法建立計時器，倒數功能將無法運作。",
    err_mutex: "無法確認是否已有另一份在執行。",

    remaining_text: zh_remaining,
    cli_hold: zh_cli_hold,
    tooltip: zh_tooltip,
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
