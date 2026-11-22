//! The cookies table is a table the user scans for what a `Set-Cookie` header actually
//! set (#230): its columns are sized to what they hold instead of sharing the row evenly,
//! an attribute the header did not set leaves its cell blank instead of filling it with a
//! placeholder, and a boolean attribute shows as a flag pill.

use gpui::{Bounds, ElementId, Entity, Pixels, TestAppContext, VisualTestContext};
use http_engine::Header;
use ui::test_support::{draw_window, init_test};

use super::CookiesTab;
use crate::child_id;

/// The cookies tab with `headers` in it, drawn once.
fn cookies_tab(
    headers: Vec<Header>,
    cx: &mut TestAppContext,
) -> (Entity<CookiesTab>, &mut VisualTestContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| CookiesTab::new(cx));
    view.update(cx, |view, cx| view.set_headers(&headers, cx));
    draw_window(cx);
    (view, cx)
}

/// Where one row's cell paints, or [`None`] where the cell is blank (the
/// `cookies_selection_tests.rs` selector, without the "it must be there" expectation).
fn cell_bounds(
    view: &Entity<CookiesTab>,
    row_key: &str,
    column: &str,
    cx: &mut VisualTestContext,
) -> Option<Bounds<Pixels>> {
    let base = ElementId::from(("cookies-tab", view.entity_id()));
    let row = child_id(&base, row_key.to_owned());
    let selector = child_id(&row, column.to_owned()).to_string();
    cx.debug_bounds(selector.leak())
}

#[gpui::test]
fn text_columns_take_the_room_the_short_columns_do_not_need(cx: &mut TestAppContext) {
    let (view, cx) = cookies_tab(
        vec![Header::new(
            "Set-Cookie",
            "sid=token-REDACTED-example; Path=/; Max-Age=60; Secure; HttpOnly",
        )],
        cx,
    );

    let name = cell_bounds(&view, "sid#0", "name", cx).expect("the name cell is painted");
    let value = cell_bounds(&view, "sid#0", "value", cx).expect("the value cell is painted");
    let max_age = cell_bounds(&view, "sid#0", "max-age", cx).expect("the Max-Age cell is painted");
    let secure = cell_bounds(&view, "sid#0", "secure", cx).expect("the Secure pill is painted");

    assert!(
        value.size.width > secure.size.width,
        "the value the user reads gets more room than a flag pill: value {:?}, secure {:?}",
        value.size,
        secure.size
    );
    assert!(
        value.size.width > max_age.size.width,
        "the value gets more room than a number column: value {:?}, max-age {:?}",
        value.size,
        max_age.size
    );
    assert!(
        name.right() < value.left(),
        "columns are padded apart, not packed edge to edge: name {name:?}, value {value:?}"
    );
}

#[gpui::test]
fn an_absent_attribute_leaves_its_cell_blank(cx: &mut TestAppContext) {
    let (view, cx) = cookies_tab(
        vec![
            Header::new("Set-Cookie", "sid=token-REDACTED-example"),
            Header::new("Set-Cookie", "theme=dark; Domain=example.com"),
        ],
        cx,
    );

    assert!(
        cell_bounds(&view, "sid#0", "domain", cx).is_none(),
        "a cookie with no Domain leaves the cell blank, not a placeholder"
    );
    assert!(
        cell_bounds(&view, "theme#0", "domain", cx).is_some(),
        "a cookie with a Domain shows it"
    );
}

#[gpui::test]
fn a_boolean_attribute_shows_as_a_flag_pill(cx: &mut TestAppContext) {
    let (view, cx) = cookies_tab(
        vec![
            Header::new("Set-Cookie", "sid=token-REDACTED-example; Secure; HttpOnly"),
            Header::new("Set-Cookie", "theme=dark"),
        ],
        cx,
    );

    for column in ["secure", "http-only"] {
        assert!(
            cell_bounds(&view, "sid#0", column, cx).is_some(),
            "a set boolean attribute shows as a flag pill in the {column} column"
        );
        assert!(
            cell_bounds(&view, "theme#0", column, cx).is_none(),
            "an unset boolean attribute leaves the {column} cell blank"
        );
    }
}
