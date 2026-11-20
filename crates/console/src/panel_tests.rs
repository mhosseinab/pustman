//! The Console panel's filters (S32d) and keyboard-reachable rows (S32d review 1):
//! entries of different levels and texts are pushed through [`ConsoleStore::push`], the
//! filters are set the way the toolbar sets them, and the assertions are on exactly the
//! entries the list shows — a panel that ignored either filter would show too many rows
//! and fail. The row test drives Enter on a focused row the way a keyboard user does.

use gpui::{Entity, FocusHandle, Focusable, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test, press_key};

use super::ConsolePanel;
use crate::{ConsoleEntry, ConsoleLevel, ConsoleStore};

/// One entry about `url`, flagged `level`, whose raw text contains `text`.
fn entry(level: ConsoleLevel, url: &str, text: &str) -> ConsoleEntry {
    ConsoleEntry {
        level,
        protocol: "http".to_string(),
        method: Some(api_model::Method::Get),
        url: url.to_string(),
        status: Some(200),
        time: Some(std::time::Duration::from_millis(12)),
        size: Some(34),
        raw_request: format!("GET {url}\n{text}"),
        raw_response: format!("200 OK\n{text}"),
    }
}

/// The buffer indices the panel's list shows right now.
fn visible_entries(panel: &Entity<ConsolePanel>, cx: &mut VisualTestContext) -> Vec<usize> {
    cx.read(|cx| panel.read_with(cx, |panel, cx| panel.visible_entries(cx)))
}

/// The focus handle of the panel's first visible row — the row a user reaches first
/// from the toolbar.
fn first_row_focus(panel: &Entity<ConsolePanel>, cx: &mut VisualTestContext) -> FocusHandle {
    cx.read(|cx| {
        panel.read_with(cx, |panel, cx| {
            let (_, key) = panel
                .visible_rows(cx)
                .first()
                .cloned()
                .expect("the pushed entry must have a visible row");
            panel
                .row_focus
                .get(&key)
                .cloned()
                .expect("every visible row keeps a focus handle")
        })
    })
}

#[gpui::test]
async fn filters_by_level_and_text(cx: &mut TestAppContext) {
    // `text_input::init` binds the editing keys, without which typing into the filter
    // field does nothing.
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
    });
    let (panel, cx) = cx.add_window_view(ConsolePanel::new);
    cx.update(|_, cx| {
        ConsoleStore::push(
            cx,
            entry(ConsoleLevel::Info, "https://api.test/users", "users list"),
        );
        ConsoleStore::push(
            cx,
            entry(
                ConsoleLevel::Error,
                "https://api.test/orders",
                "orders failed",
            ),
        );
        ConsoleStore::push(
            cx,
            entry(
                ConsoleLevel::Error,
                "https://api.test/users/export",
                "users export failed",
            ),
        );
    });
    cx.run_until_parked();

    // The level filter alone: only the error entries survive it.
    panel.update(cx, |panel, cx| {
        panel.set_level_filter(Some(ConsoleLevel::Error), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        visible_entries(&panel, cx),
        vec![1, 2],
        "the level filter must drop entries of other levels"
    );

    // The text filter is driven the way the user drives it: focus the field and type.
    let text_focus =
        cx.read(|cx| panel.read_with(cx, |panel, cx| panel.text_filter.read(cx).focus_handle(cx)));
    cx.update(|window, cx| window.focus(&text_focus, cx));
    draw_window(cx);
    cx.simulate_input("users");
    cx.run_until_parked();

    // Together they show exactly the one entry that is both an error and about users —
    // ignoring either filter would also show the users info entry or the orders error.
    assert_eq!(
        visible_entries(&panel, cx),
        vec![2],
        "the level and text filters must both be applied to the list"
    );
}

#[gpui::test]
async fn enter_on_a_focused_row_expands_its_details(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
    });
    let (panel, cx) = cx.add_window_view(ConsolePanel::new);
    cx.update(|_, cx| {
        ConsoleStore::push(
            cx,
            entry(ConsoleLevel::Info, "https://api.test/users", "users list"),
        );
    });
    cx.run_until_parked();
    draw_window(cx);

    // The row is focused the way the toolbar buttons are focused — through its own
    // focus handle — and Enter is what a keyboard user presses.
    let row_focus = first_row_focus(&panel, cx);
    cx.update(|window, cx| window.focus(&row_focus, cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();

    let expanded = cx.read(|cx| panel.read_with(cx, |panel, _| panel.expanded));
    assert_eq!(
        expanded,
        Some(0),
        "Enter on a focused row must expand its raw request and response details"
    );
}
