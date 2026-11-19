//! Shared test helpers for this crate and for other crates' tests (via the
//! `test-support` feature): the app's real init order, and the two GPUI test mechanics
//! the component tests repeat — painting the window once and pressing a key the way a
//! user does.

use gpui::{App, KeyDownEvent, KeyUpEvent, Keystroke, VisualTestContext};

use settings::SettingsStore;

/// Run the app's init order — `settings → theme → ui`, the order `main.rs` runs it in —
/// over the bundled defaults. A test then reads the real registered settings, theme and
/// design system instead of a hand-rolled approximation. A failure here is a broken
/// test fixture, so it panics with the parser's own message.
pub fn init_test(cx: &mut App) {
    if let Err(error) = SettingsStore::init(cx, None) {
        panic!("the bundled settings must parse: {error}");
    }
    if let Err(error) = theme::init(cx) {
        panic!("the bundled themes must parse: {error}");
    }
    crate::init(cx);
}

/// Paint the window's current view tree once. GPUI test windows never paint themselves,
/// and hitboxes (for simulated clicks) and focus-dependent key handlers only register
/// during a paint.
pub fn draw_window(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

/// Press and release one key, the way a user does.
///
/// [`VisualTestContext::simulate_keystrokes`] dispatches only the key down, while GPUI
/// turns a full press-and-release of Enter or Space on a focused control into exactly one
/// click — the keyboard's side of [`crate::components::behavior::Clickable`].
pub fn press_key(cx: &mut VisualTestContext, key: &str) {
    let os = cx.read(|cx| cx.platform_info().os);
    let keystroke = match Keystroke::parse_with_os(key, os) {
        Ok(keystroke) => keystroke,
        Err(error) => panic!("the test key {key:?} must parse: {error}"),
    };
    cx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(KeyUpEvent { keystroke });
}
