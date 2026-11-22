//! The Console toolbar's level filter and pause control (#232): the level filters are
//! one segmented control — chips inset in one rounded track, the selected one filled by
//! the design system's segmented style — and pause is an icon button with a tooltip,
//! like the clear button beside it, not a text button.
//!
//! `Decision:` the level filter is the design system's segmented switch
//! (`ui::TabBar::segmented()`), the control the response body's Pretty/Raw/Hex/Preview
//! switch already draws, so "which level shows" is one single-select control with a
//! filled selected chip instead of four loose text buttons (S02/R02). The selected
//! chip's fill is `TabStyle::Segmented`'s own, pinned in `ui`'s
//! `tab_bar_segmented_tests.rs` ("the selected segment is filled, not underlined");
//! these tests pin that the console toolbar draws that control at all.
//! `Decision:` Pause is an icon button whose tooltip is its name — the accessible label
//! an icon-only control must carry — and which is drawn the size of the clear button
//! beside it (S26/R26). The selector names below are spelled out as literals (they are
//! `debug_selector` values in `panel.rs`) so this test compiles — and fails — against
//! the toolbar as it stands.

use std::time::Duration;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, px};
use ui::SEGMENTED_TRACK_SELECTOR;
use ui::components::tooltip::TOOLTIP_SELECTOR;
use ui::test_support::{draw_window, init_test};

use super::ConsolePanel;

/// The level segments, in bar order.
const LEVEL_SEGMENTS: [&str; 4] = [
    "console-filter-all",
    "console-filter-info",
    "console-filter-warning",
    "console-filter-error",
];
/// The control around the pause button, and the one around the clear button the test
/// sizes it against.
const PAUSE_CONTROL: &str = "console-pause-control";
const CLEAR_CONTROL: &str = "console-clear-control";

/// Longer than GPUI's tooltip show delay (500 ms).
const PAST_TOOLTIP_DELAY: Duration = Duration::from_millis(600);

/// A console panel over the real settings, theme and store, drawn once.
fn console_panel(cx: &mut TestAppContext) -> (Entity<ConsolePanel>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
    });
    cx.add_window_view(ConsolePanel::new)
}

#[gpui::test]
async fn level_filters_are_one_segmented_control(cx: &mut TestAppContext) {
    let (_panel, cx) = console_panel(cx);
    draw_window(cx);

    let track = cx
        .debug_bounds(SEGMENTED_TRACK_SELECTOR)
        .expect("the level filters share one segmented track");
    for selector in LEVEL_SEGMENTS {
        let segment = cx
            .debug_bounds(selector)
            .expect("every level filter is drawn as a segment of that control");
        assert!(
            segment.top() > track.top() && segment.bottom() < track.bottom(),
            "{selector} is a chip inset in the shared track: {segment:?} in {track:?}"
        );
        assert!(
            segment.left() >= track.left() - px(0.5) && segment.right() <= track.right(),
            "{selector} lies inside the track"
        );
    }
}

#[gpui::test]
async fn pause_is_an_icon_button_that_explains_itself_on_hover(cx: &mut TestAppContext) {
    let (_panel, cx) = console_panel(cx);
    draw_window(cx);

    let pause = cx
        .debug_bounds(PAUSE_CONTROL)
        .expect("the pause control is drawn");
    let clear = cx
        .debug_bounds(CLEAR_CONTROL)
        .expect("the clear control is drawn");
    assert_eq!(
        pause.size, clear.size,
        "Pause is drawn as an icon button like Clear beside it, not as a text button"
    );

    cx.simulate_mouse_move(pause.center(), None, Modifiers::none());
    draw_window(cx);
    cx.executor().advance_clock(PAST_TOOLTIP_DELAY);
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.debug_bounds(TOOLTIP_SELECTOR).is_some(),
        "the icon-only pause button names itself in a tooltip"
    );
}
