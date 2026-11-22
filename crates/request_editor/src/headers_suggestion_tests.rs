//! The Headers tab's suggestion keystrokes (REQ-HTTP-03, #152): the keyboard flow over
//! the suggestion list — Up/Down choose, Enter accepts, Escape dismisses — driven the
//! way a user drives it, and the muted hint the list closes with. The step this code
//! landed in (S27b) bound one test for it; this file is the coverage the keystroke
//! path needed.
//!
//! `Decision:` the styling is asserted through [`Headers::suggestion_hint_color`] —
//! the value `render` hands the hint label — because GPUI's test support exposes
//! layout (`debug_bounds`) and painted quads but not text colours (the same reason
//! `crate::url_bar_method_color_tests` records): what a test asserts here is what the
//! user sees (S03/R03, `references/design_system.md`: secondary metadata draws in the
//! muted text token).

use std::collections::BTreeMap;

use api_model::{HttpRequest, KeyValue};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use super::Headers;
use crate::RequestEditor;
use crate::key_value_table::RowField;

/// A row with no key yet — the row a user starts typing a header name into.
fn empty_row() -> KeyValue {
    KeyValue {
        key: String::new(),
        value: None,
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// A draft GET with one unstarted header row, the way a request tab opens.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/items");
    request.headers = vec![empty_row()];
    request
}

/// A request tab on the app's real init order, with the Headers tab open and the
/// cursor in the row's key cell — where a user types a header name.
fn open_editor(cx: &mut TestAppContext) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request(), window, cx));
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    let tab = cx
        .debug_bounds("request-tab-headers")
        .expect("the Headers tab is in the request tab strip");
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
    let table =
        cx.read(|cx| editor.read_with(cx, |editor, cx| editor.headers().read(cx).table().clone()));
    let row_id = cx.read(|cx| {
        editor
            .read_with(cx, |editor, cx| {
                editor.headers().read(cx).table().read(cx).row_ids()
            })
            .first()
            .copied()
    });
    let Some(row_id) = row_id else {
        panic!("the request opens on one header row");
    };
    cx.update(|window, cx| {
        table.update(cx, |table, cx| {
            table.focus_cell(row_id, RowField::Key, window, cx)
        })
    });
    draw_window(cx);
    (editor, cx)
}

/// The header names as the tab stores them, in row order.
fn header_names(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor
                .headers()
                .read(cx)
                .rows(cx)
                .into_iter()
                .map(|row| row.key)
                .collect()
        })
    })
}

/// Whether the name-suggestion list is on screen.
fn name_suggestions_shown(cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds("header-name-suggestions").is_some()
}

/// Type `text` into the focused key cell and let the suggestion list open.
fn type_key(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    cx.run_until_parked();
    draw_window(cx);
}

#[gpui::test]
fn test_enter_accepts_the_suggestion_the_list_opened_on(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);
    type_key("Acc", cx);
    assert!(
        name_suggestions_shown(cx),
        "typing a header-name prefix opens the suggestion list"
    );

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        header_names(&editor, cx),
        vec![String::from("Accept")],
        "Enter puts the highlighted suggestion into the key cell"
    );
    assert!(
        !name_suggestions_shown(cx),
        "and closes the list once it is accepted"
    );
}

#[gpui::test]
fn test_down_moves_the_selection_and_enter_accepts_it(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);
    type_key("Acc", cx);

    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        header_names(&editor, cx),
        vec![String::from("Accept-Encoding")],
        "Down moves the selection to the next suggestion, and Enter accepts that one"
    );
}

#[gpui::test]
fn test_up_wraps_the_selection_to_the_last_suggestion(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);
    type_key("Acc", cx);

    cx.simulate_keystrokes("up enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        header_names(&editor, cx),
        vec![String::from("Accept-Language")],
        "Up from the first suggestion wraps to the last one"
    );
}

#[gpui::test]
fn test_escape_dismisses_the_suggestions_and_keeps_the_raw_text(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);
    type_key("Acc", cx);
    assert!(
        name_suggestions_shown(cx),
        "typing a header-name prefix opens the suggestion list"
    );

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        !name_suggestions_shown(cx),
        "Escape closes the suggestion list"
    );
    assert_eq!(
        header_names(&editor, cx),
        vec![String::from("Acc")],
        "and leaves exactly what the user typed in the cell"
    );
}

#[gpui::test]
fn test_the_suggestion_hint_draws_muted(cx: &mut TestAppContext) {
    cx.update(init_test);
    assert_eq!(
        Headers::suggestion_hint_color(),
        Color::Muted,
        "the keystroke hint under the suggestions draws muted, one step back from them"
    );
}
