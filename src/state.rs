//! Application state machine. This module is deliberately pure: it pulls in no
//! Win32 types, so every decision the program makes lives here and is covered
//! by plain `cargo test`.

use std::time::{Duration, Instant};

use crate::i18n::Strings;

/// How long a hold should last. `Forever` means no limit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Span {
    Forever,
    Minutes(u32),
}

impl Span {
    pub fn as_duration(self) -> Option<Duration> {
        match self {
            Span::Forever => None,
            Span::Minutes(m) => Some(Duration::from_secs(m as u64 * 60)),
        }
    }
}

/// The durations offered in the menu, in display order.
///
/// Labels live in [`crate::i18n::Strings::spans`]; the two must stay aligned by
/// index, which is what the menu item ids encode.
pub const SPANS: [Span; 8] = [
    Span::Forever,
    Span::Minutes(5),
    Span::Minutes(10),
    Span::Minutes(15),
    Span::Minutes(30),
    Span::Minutes(60),
    Span::Minutes(120),
    Span::Minutes(300),
];

/// The complete application state. Fields are private on purpose: everything
/// goes through the methods below so `deadline` can never drift out of sync
/// with the switches and the selected span.
#[derive(Clone, Copy, Debug)]
pub struct AppState {
    system: bool,
    display: bool,
    span: Span,
    deadline: Option<Instant>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            system: false,
            display: false,
            span: Span::Forever,
            deadline: None,
        }
    }

    pub fn system(&self) -> bool {
        self.system
    }

    pub fn display(&self) -> bool {
        self.display
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn is_active(&self) -> bool {
        self.system || self.display
    }

    pub fn toggle_system(&mut self, now: Instant) {
        let was_active = self.is_active();
        self.system = !self.system;
        self.settle(was_active, now);
    }

    pub fn toggle_display(&mut self, now: Instant) {
        let was_active = self.is_active();
        self.display = !self.display;
        self.settle(was_active, now);
    }

    pub fn set_span(&mut self, span: Span, now: Instant) {
        self.span = span;
        self.deadline = if self.is_active() {
            span.as_duration().map(|d| now + d)
        } else {
            None
        };
    }

    /// If the countdown has expired, reset everything and return `true` so the
    /// caller knows to refresh the UI.
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.deadline {
            Some(deadline) if now >= deadline => {
                self.system = false;
                self.display = false;
                self.span = Span::Forever;
                self.deadline = None;
                true
            }
            _ => false,
        }
    }

    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.deadline.map(|d| d.saturating_duration_since(now))
    }

    /// Fix up `deadline` after a switch was toggled. Three cases:
    ///
    /// 1. Everything is now off, so clear the deadline.
    /// 2. We just went from inactive to active, so start the countdown.
    /// 3. We were already active and still are, so keep the existing deadline.
    ///    Turning on a second switch must not restart the clock.
    fn settle(&mut self, was_active: bool, now: Instant) {
        if !self.is_active() {
            self.deadline = None;
        } else if !was_active {
            self.deadline = self.span.as_duration().map(|d| now + d);
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Format a duration as `HH:MM:SS`. Values past 99 hours are not special-cased;
/// the longest span offered is five hours.
pub fn format_hms(d: Duration) -> String {
    let total = d.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

impl AppState {
    /// Text for the "remaining" row in the menu.
    pub fn remaining_text(&self, now: Instant, s: &Strings) -> String {
        match self.remaining(now) {
            Some(d) => (s.remaining_text)(&format_hms(d)),
            None => s.no_timer.to_string(),
        }
    }

    /// Text for the tray icon's tooltip.
    pub fn tooltip(&self, now: Instant, s: &Strings) -> String {
        let what = match (self.system, self.display) {
            (false, false) => return (s.tooltip)(s.tip_off, None),
            (true, true) => s.tip_both,
            (true, false) => s.tip_system,
            (false, true) => s.tip_screen,
        };
        let hms = self.remaining(now).map(format_hms);
        (s.tooltip)(what, hms.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every test works from one fixed origin rather than waiting on real time.
    fn t0() -> Instant {
        Instant::now()
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn starts_inactive() {
        let s = AppState::new();
        assert!(!s.system());
        assert!(!s.display());
        assert_eq!(s.span(), Span::Forever);
        assert!(!s.is_active());
        assert_eq!(s.remaining(t0()), None);
    }

    #[test]
    fn forever_span_produces_no_countdown() {
        let now = t0();
        let mut s = AppState::new();
        s.toggle_system(now);
        assert!(s.system());
        assert!(s.is_active());
        assert_eq!(s.remaining(now), None);
    }

    #[test]
    fn setting_a_span_while_inactive_does_not_start_the_clock() {
        let now = t0();
        let mut s = AppState::new();
        s.set_span(Span::Minutes(30), now);
        assert_eq!(s.span(), Span::Minutes(30));
        assert_eq!(s.remaining(now), None, "nothing is on, so nothing counts down");
    }

    #[test]
    fn the_clock_starts_when_a_switch_goes_on() {
        let now = t0();
        let mut s = AppState::new();
        s.set_span(Span::Minutes(30), now);
        s.toggle_system(now);
        assert_eq!(s.remaining(now), Some(secs(1800)));
    }

    #[test]
    fn a_second_switch_does_not_restart_the_clock() {
        let now = t0();
        let mut s = AppState::new();
        s.set_span(Span::Minutes(30), now);
        s.toggle_system(now);
        let later = now + secs(600);
        s.toggle_display(later);
        assert_eq!(
            s.remaining(later),
            Some(secs(1200)),
            "already counting down; a second switch must not reset it"
        );
    }

    #[test]
    fn changing_the_span_mid_countdown_restarts_it() {
        let now = t0();
        let mut s = AppState::new();
        s.set_span(Span::Minutes(30), now);
        s.toggle_system(now);
        let later = now + secs(600);
        s.set_span(Span::Minutes(60), later);
        assert_eq!(s.remaining(later), Some(secs(3600)));
    }

    #[test]
    fn switching_back_to_forever_clears_the_countdown() {
        let now = t0();
        let mut s = AppState::new();
        s.set_span(Span::Minutes(30), now);
        s.toggle_system(now);
        s.set_span(Span::Forever, now);
        assert_eq!(s.remaining(now), None);
        assert!(s.is_active(), "changing the span must not touch the switches");
    }

    #[test]
    fn turning_everything_off_clears_the_countdown() {
        let now = t0();
        let mut s = AppState::new();
        s.set_span(Span::Minutes(30), now);
        s.toggle_system(now);
        s.toggle_display(now);
        s.toggle_system(now);
        assert_eq!(
            s.remaining(now),
            Some(secs(1800)),
            "one switch is still on, so the countdown continues"
        );
        s.toggle_display(now);
        assert!(!s.is_active());
        assert_eq!(s.remaining(now), None);
    }

    #[test]
    fn tick_does_nothing_before_the_deadline() {
        let now = t0();
        let mut s = AppState::new();
        s.set_span(Span::Minutes(30), now);
        s.toggle_system(now);
        assert!(!s.tick(now + secs(1799)));
        assert!(s.is_active());
    }

    #[test]
    fn tick_resets_everything_at_the_deadline() {
        let now = t0();
        let mut s = AppState::new();
        s.set_span(Span::Minutes(30), now);
        s.toggle_system(now);
        s.toggle_display(now);
        let expiry = now + secs(1800);
        assert!(s.tick(expiry), "expiry should report true so the UI refreshes");
        assert!(!s.system());
        assert!(!s.display());
        assert_eq!(s.span(), Span::Forever, "the span returns to Forever");
        assert_eq!(s.remaining(expiry), None);
        assert!(!s.tick(expiry), "already reset; must not report again");
    }

    #[test]
    fn tick_never_fires_in_forever_mode() {
        let now = t0();
        let mut s = AppState::new();
        s.toggle_system(now);
        assert!(!s.tick(now + secs(86_400)));
        assert!(s.is_active());
    }

    #[test]
    fn remaining_saturates_at_zero() {
        let now = t0();
        let mut s = AppState::new();
        s.set_span(Span::Minutes(30), now);
        s.toggle_system(now);
        assert_eq!(s.remaining(now + secs(3600)), Some(Duration::ZERO));
    }

    #[test]
    fn format_hms_pads_to_two_digits() {
        assert_eq!(format_hms(Duration::ZERO), "00:00:00");
        assert_eq!(format_hms(secs(59)), "00:00:59");
        assert_eq!(format_hms(secs(60)), "00:01:00");
        assert_eq!(format_hms(secs(3661)), "01:01:01");
        assert_eq!(format_hms(secs(18000)), "05:00:00");
    }

    #[test]
    fn spans_match_the_intended_menu() {
        assert_eq!(
            SPANS,
            [
                Span::Forever,
                Span::Minutes(5),
                Span::Minutes(10),
                Span::Minutes(15),
                Span::Minutes(30),
                Span::Minutes(60),
                Span::Minutes(120),
                Span::Minutes(300),
            ]
        );
    }

    #[test]
    fn every_language_has_a_label_per_span() {
        // The two are matched by index; a length mismatch would wire a menu
        // entry to the wrong duration.
        assert_eq!(crate::i18n::EN.spans.len(), SPANS.len());
        assert_eq!(crate::i18n::ZH.spans.len(), SPANS.len());
    }

    #[test]
    fn remaining_text_in_english() {
        let now = t0();
        let s = &crate::i18n::EN;
        let mut st = AppState::new();
        assert_eq!(st.remaining_text(now, s), "No timer");
        st.set_span(Span::Minutes(60), now);
        st.toggle_display(now);
        assert_eq!(st.remaining_text(now, s), "01:00:00 remaining");
    }

    #[test]
    fn remaining_text_in_chinese() {
        let now = t0();
        let s = &crate::i18n::ZH;
        let mut st = AppState::new();
        assert_eq!(st.remaining_text(now, s), "未計時");
        st.set_span(Span::Minutes(60), now);
        st.toggle_display(now);
        assert_eq!(st.remaining_text(now, s), "剩餘 01:00:00");
    }

    #[test]
    fn tooltip_in_english() {
        let now = t0();
        let s = &crate::i18n::EN;
        let mut st = AppState::new();
        assert_eq!(st.tooltip(now, s), "caffeinate: off");
        st.toggle_system(now);
        assert_eq!(st.tooltip(now, s), "caffeinate: system");
        st.toggle_display(now);
        assert_eq!(st.tooltip(now, s), "caffeinate: system + screen");
        st.toggle_system(now);
        assert_eq!(st.tooltip(now, s), "caffeinate: screen");
    }

    #[test]
    fn tooltip_in_chinese() {
        let now = t0();
        let s = &crate::i18n::ZH;
        let mut st = AppState::new();
        assert_eq!(st.tooltip(now, s), "caffeinate：未啟用");
        st.toggle_system(now);
        assert_eq!(st.tooltip(now, s), "caffeinate：系統");
        st.toggle_display(now);
        assert_eq!(st.tooltip(now, s), "caffeinate：系統+螢幕");
        st.toggle_system(now);
        assert_eq!(st.tooltip(now, s), "caffeinate：螢幕");
    }

    #[test]
    fn tooltip_includes_the_remaining_time() {
        let now = t0();
        let mut st = AppState::new();
        st.set_span(Span::Minutes(30), now);
        st.toggle_system(now);
        assert_eq!(
            st.tooltip(now, &crate::i18n::EN),
            "caffeinate: system, 00:30:00 remaining"
        );
        assert_eq!(
            st.tooltip(now, &crate::i18n::ZH),
            "caffeinate：系統 · 剩餘 00:30:00"
        );
    }
}
