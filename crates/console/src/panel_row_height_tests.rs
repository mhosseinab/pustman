//! The Console list rows and the details region have a real height. Both were sized with
//! `px_to_rems` on a value already in rems, so a row drew 1.75 px tall and every entry's
//! text smeared over its neighbours, and the details region drew 14 px tall.

use gpui::{TestAppContext, px};
use ui::test_support::{draw_window, init_test};

use super::{ConsolePanel, DETAILS_SELECTOR, ENTRY_ROW_SELECTOR};
use crate::{ConsoleEntry, ConsoleLevel, ConsoleStore};

fn entry(url: &str) -> ConsoleEntry {
    ConsoleEntry {
        level: ConsoleLevel::Info,
        protocol: "http".to_string(),
        method: Some(api_model::Method::Get),
        url: url.to_string(),
        status: Some(200),
        time: Some(std::time::Duration::from_millis(12)),
        size: Some(34),
        raw_request: format!("GET {url}"),
        raw_response: "200 OK".to_string(),
    }
}

#[gpui::test]
async fn test_console_rows_are_tall_enough_for_a_line_of_text(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
        ConsoleStore::push(cx, entry("https://example.com/one"));
        ConsoleStore::push(cx, entry("https://example.com/two"));
    });
    let (_panel, cx) = cx.add_window_view(ConsolePanel::new);
    draw_window(cx);

    let row = cx
        .debug_bounds(ENTRY_ROW_SELECTOR)
        .expect("an entry row is drawn");
    assert!(
        row.size.height >= px(24.),
        "a console row is {:?} tall, too short for its text",
        row.size.height
    );
}

#[gpui::test]
async fn test_expanded_details_region_has_room_for_the_raw_text(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
        ConsoleStore::push(cx, entry("https://example.com/one"));
    });
    let (panel, cx) = cx.add_window_view(ConsolePanel::new);
    panel.update(cx, |panel, cx| panel.toggle_expanded(0, cx));
    draw_window(cx);

    let details = cx
        .debug_bounds(DETAILS_SELECTOR)
        .expect("the details region is drawn");
    assert!(
        details.size.height >= px(160.),
        "the details region is {:?} tall",
        details.size.height
    );
}
