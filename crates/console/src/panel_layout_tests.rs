//! The Console panel's layout: the text filter is wide enough to type in, and the empty
//! state sits in the middle of the entries area rather than pinned under the toolbar.
//! Both are measured from the drawn frame through the debug selectors, so a width or a
//! container that collapses fails here even though every filter test still passes.

use gpui::{TestAppContext, px};
use ui::test_support::{draw_window, init_test};

use super::{ConsolePanel, EMPTY_STATE_SELECTOR, ENTRIES_AREA_SELECTOR, TEXT_FILTER_SELECTOR};

#[gpui::test]
async fn text_filter_is_wide_enough_to_type_a_query(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
    });
    let (_panel, cx) = cx.add_window_view(ConsolePanel::new);
    draw_window(cx);

    let filter = cx
        .debug_bounds(TEXT_FILTER_SELECTOR)
        .expect("the text filter is drawn");
    // A query like "users/export" needs well over a hundred pixels; the field once
    // collapsed to 14 px and showed a single clipped character.
    assert!(
        filter.size.width >= px(160.),
        "the text filter is {:?} wide",
        filter.size.width
    );
}

#[gpui::test]
async fn empty_console_message_is_centered_in_the_entries_area(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
    });
    let (_panel, cx) = cx.add_window_view(ConsolePanel::new);
    draw_window(cx);

    let area = cx
        .debug_bounds(ENTRIES_AREA_SELECTOR)
        .expect("the entries area is drawn");
    let message = cx
        .debug_bounds(EMPTY_STATE_SELECTOR)
        .expect("the empty state is drawn");
    let offset = (area.center().y - message.center().y).abs();
    assert!(
        offset <= px(2.),
        "the empty state sits {offset:?} off the middle of the entries area"
    );
}
