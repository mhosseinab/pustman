//! The expanded row and ring-buffer eviction (#135): `expanded` names a buffer index, and
//! a push over the cap drops the oldest entry and shifts every index — so the expansion
//! must follow its own entry to its new index, and be dropped when that entry is gone,
//! never left showing whatever now sits at the old index.

use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use super::{ConsolePanel, DETAILS_SELECTOR};
use crate::{ConsoleEntry, ConsoleLevel, ConsoleStore};

/// One entry about `url`, whose raw text names it.
fn entry(url: &str) -> ConsoleEntry {
    ConsoleEntry {
        level: ConsoleLevel::Info,
        protocol: "http".to_string(),
        method: Some(api_model::Method::Get),
        url: url.to_string(),
        status: Some(200),
        time: Some(Duration::from_millis(12)),
        size: Some(34),
        raw_request: format!("GET {url} HTTP/1.1\r\n\r\n"),
        raw_response: format!("HTTP/1.1 200 OK\r\n\r\n{url}"),
    }
}

/// A console panel over a store capped at two entries — every push after the second
/// evicts the oldest (REQ-CON-02) — with `first` and `second` pushed into it.
fn capped_panel(cx: &mut TestAppContext) -> (Entity<ConsolePanel>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        // The cap is read when the store is installed, so it is set first (REQ-CON-02).
        settings::SettingsStore::reload(cx, Some(r#"{"console_max_entries": 2}"#))
            .expect("the cap is a setting the user can change");
        text_input::init(cx);
        crate::init(cx);
    });
    let (panel, cx) = cx.add_window_view(ConsolePanel::new);
    cx.update(|_, cx| {
        ConsoleStore::push(cx, entry("https://api.test/first"));
        ConsoleStore::push(cx, entry("https://api.test/second"));
    });
    cx.run_until_parked();
    (panel, cx)
}

/// The buffer index the panel's details region is showing an entry for.
fn expanded(panel: &Entity<ConsolePanel>, cx: &mut VisualTestContext) -> Option<usize> {
    cx.read(|cx| panel.read_with(cx, |panel, _| panel.expanded))
}

#[gpui::test]
async fn eviction_drops_the_expansion_instead_of_showing_another_entry(cx: &mut TestAppContext) {
    let (panel, cx) = capped_panel(cx);
    // The first entry is the one expanded — the one the next two pushes evict.
    panel.update(cx, |panel, cx| panel.toggle_expanded(0, cx));
    draw_window(cx);
    assert!(
        cx.debug_bounds(DETAILS_SELECTOR).is_some(),
        "the expanded entry's details are drawn"
    );

    cx.update(|_, cx| {
        ConsoleStore::push(cx, entry("https://api.test/third"));
        ConsoleStore::push(cx, entry("https://api.test/fourth"));
    });
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        expanded(&panel, cx),
        None,
        "an evicted entry's expansion is dropped, never moved onto the entry that now sits at its index"
    );
    assert!(
        cx.debug_bounds(DETAILS_SELECTOR).is_none(),
        "no details are drawn once the expanded entry is gone"
    );
}

#[gpui::test]
async fn expansion_follows_its_entry_when_eviction_shifts_the_buffer(cx: &mut TestAppContext) {
    let (panel, cx) = capped_panel(cx);
    // The second entry is the one expanded — the push below evicts the first and moves
    // the expanded entry from index 1 to index 0.
    panel.update(cx, |panel, cx| panel.toggle_expanded(1, cx));
    cx.update(|_, cx| ConsoleStore::push(cx, entry("https://api.test/third")));
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        expanded(&panel, cx),
        Some(0),
        "the expansion follows its entry to the index eviction moved it to"
    );
    assert!(
        cx.debug_bounds(DETAILS_SELECTOR).is_some(),
        "an entry that is still in the buffer keeps its details on screen"
    );
}
