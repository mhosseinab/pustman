//! The Headers and Cookies tables draw their column headings and their text cells
//! through one shared renderer ([`crate::table_cells`]), not one copy each (S30e review 1
//! finding 3): the shared helpers paint under the same debug keys in both tables, so a
//! change to a table cell lands once and every table answers the same test hooks.

use gpui::{ElementId, Entity, TestAppContext, VisualTestContext};
use http_engine::Header;
use ui::test_support::{draw_window, init_test};

use crate::child_id;
use crate::cookies_tab::CookiesTab;
use crate::headers_view::HeadersView;

/// Whether anything is on screen under this debug key (the `error_panel_titles_tests.rs`
/// pattern: `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector is
/// leaked — fine for a test's short life).
fn shown(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}

/// The headers table, with one header field in it.
fn headers_view(cx: &mut TestAppContext) -> (Entity<HeadersView>, &mut VisualTestContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| HeadersView::new(cx));
    view.update(cx, |view, cx| {
        view.set_headers(&[Header::new("Content-Type", "application/json")], cx)
    });
    draw_window(cx);
    (view, cx)
}

/// The cookies tab, with one cookie in it.
fn cookies_tab(cx: &mut TestAppContext) -> (Entity<CookiesTab>, &mut VisualTestContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| CookiesTab::new(cx));
    view.update(cx, |view, cx| {
        view.set_headers(
            &[Header::new(
                "Set-Cookie",
                "sid=token-REDACTED-example; Path=/",
            )],
            cx,
        )
    });
    draw_window(cx);
    (view, cx)
}

#[gpui::test]
fn the_headers_table_draws_its_column_headings_through_the_shared_helper(cx: &mut TestAppContext) {
    let (_view, cx) = headers_view(cx);

    for heading in ["Name", "Value"] {
        assert!(
            shown(cx, format!("column-heading-{heading}")),
            "the headers table draws its {heading} heading through `table_cells::column_heading`"
        );
    }
}

#[gpui::test]
fn the_cookies_table_draws_its_column_headings_through_the_shared_helper(cx: &mut TestAppContext) {
    let (_view, cx) = cookies_tab(cx);

    for heading in [
        "Name", "Value", "Domain", "Path", "Expires", "Max-Age", "Secure", "HttpOnly", "SameSite",
    ] {
        assert!(
            shown(cx, format!("column-heading-{heading}")),
            "the cookies tab draws its {heading} heading through `table_cells::column_heading`"
        );
    }
}

#[gpui::test]
fn both_tables_paint_a_cell_under_the_id_their_table_passes(cx: &mut TestAppContext) {
    let (view, cx) = headers_view(cx);
    let base = ElementId::from(("headers-view", view.entity_id()));
    let row = child_id(&base, "Content-Type#0");
    for column in ["name", "value"] {
        assert!(
            shown(cx, child_id(&row, column).to_string()),
            "the headers table paints its {column} cell under the id it passes in"
        );
    }

    let (view, cx) = cookies_tab(cx);
    let base = ElementId::from(("cookies-tab", view.entity_id()));
    let row = child_id(&base, "sid#0");
    for column in ["name", "value"] {
        assert!(
            shown(cx, child_id(&row, column).to_string()),
            "the cookies tab paints its {column} cell under the id it passes in"
        );
    }
}
