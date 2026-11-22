//! The request editor's section titles stand a clear step above the table column
//! headers (#222): "Query Params", "Path Variables", "Headers" and "Automatic Headers"
//! paint at the design system's section-title size in the primary text colour — one
//! step up from the 12 px muted column headers they used to be indistinguishable from.
//! Every assertion is on what paints.

use gpui::{Bounds, Entity, Modifiers, Pixels, TestAppContext, VisualTestContext, px};
use ui::components::table::table_header_selector;
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;
use crate::headers::{AUTO_HEADERS_SECTION_TITLE_SELECTOR, HEADERS_SECTION_TITLE_SELECTOR};
use crate::params::{PATH_SECTION_TITLE_SELECTOR, QUERY_SECTION_TITLE_SELECTOR};

/// The painted height a section title reaches at the pack's 16/24 section-title size
/// (16 px type paints about 26 px tall on the default scale), and the gap that reads as
/// "one step up" over a 12 px column header (about 20 px tall).
const SECTION_TITLE_MIN_HEIGHT: Pixels = px(24.);
const MIN_STEP_ABOVE_HEADERS: Pixels = px(3.);

/// A draft GET to `url`, the way a request tab opens.
fn request(url: &str) -> api_model::HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from(url);
    request
}

/// A request tab on the app's real init order, showing the Params tab it opens on.
fn open_editor(
    cx: &mut TestAppContext,
    request: api_model::HttpRequest,
) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (editor, cx)
}

/// Open one tab of the request tab strip the way a user does: click it.
fn open_tab(tab_id: &'static str, cx: &mut VisualTestContext) {
    let tab = cx
        .debug_bounds(tab_id)
        .unwrap_or_else(|| panic!("{tab_id} is in the request tab strip"));
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// The painted bounds under `selector` (`debug_bounds` takes `&'static str`, so the
/// built key is leaked — fine for a test's short life).
fn bounds(selector: String, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let selector: &'static str = selector.leak();
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"))
}

/// Assert `title` paints at the section-title size and one clear step above the
/// column header `header`.
fn assert_title_stands_above(title: Bounds<Pixels>, header: Bounds<Pixels>, what: &str) {
    assert!(
        title.size.height >= SECTION_TITLE_MIN_HEIGHT,
        "{what} paints at the 16 px section-title size: {title:?}"
    );
    assert!(
        title.size.height > header.size.height + MIN_STEP_ABOVE_HEADERS,
        "{what} stands one step above the column headers: title {title:?}, header {header:?}"
    );
}

#[gpui::test]
fn test_query_params_title_stands_above_the_column_headers(cx: &mut TestAppContext) {
    let (_editor, cx) = open_editor(cx, request("https://example.com/ping"));
    let title = bounds(String::from(QUERY_SECTION_TITLE_SELECTOR), cx);
    let header = bounds(table_header_selector("Key"), cx);

    assert_title_stands_above(title, header, "the Query Params section title");
}

#[gpui::test]
fn test_path_variables_title_stands_above_the_column_headers(cx: &mut TestAppContext) {
    let (_editor, cx) = open_editor(cx, request("https://example.com/users/:id"));
    let title = bounds(String::from(PATH_SECTION_TITLE_SELECTOR), cx);
    let header = bounds(table_header_selector("Key"), cx);

    assert_title_stands_above(title, header, "the Path Variables section title");
}

#[gpui::test]
fn test_headers_titles_stand_above_the_column_headers(cx: &mut TestAppContext) {
    let (_editor, cx) = open_editor(cx, request("https://example.com/ping"));
    open_tab("request-tab-headers", cx);
    let header = bounds(table_header_selector("Key"), cx);

    for (selector, what) in [
        (HEADERS_SECTION_TITLE_SELECTOR, "the Headers section title"),
        (
            AUTO_HEADERS_SECTION_TITLE_SELECTOR,
            "the Automatic Headers section title",
        ),
    ] {
        assert_title_stands_above(bounds(String::from(selector), cx), header, what);
    }
}
