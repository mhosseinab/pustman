//! The response body's line-number gutter (#225): every body line draws its number in a
//! monospace gutter left of the code, one shared column wide so the code lines up however
//! many digits the body needs.
//!
//! The blueprint's code view numbers its lines (`06_visuals/workbench_blueprint.svg`:
//! `1 {`, `2   "data": [`). Bare text rows make a long body impossible to talk about —
//! there is no way to point at "line 214".

use std::sync::Arc;

use gpui::{Bounds, ElementId, Entity, Pixels, TestAppContext, VisualTestContext};
use http_engine::{Header, Response, ResponseBody};
use ui::test_support::{draw_window, init_test};

use super::{BodyMode, BodyView};
use crate::child_id;

/// The pane under test: Raw mode, `line_count` plain lines on screen.
fn raw_body_view(
    line_count: usize,
    cx: &mut TestAppContext,
) -> (Entity<BodyView>, &mut VisualTestContext) {
    cx.update(init_test);
    let body = (0..line_count)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(body.into_bytes()));
    response.headers = vec![Header::new("content-type", "text/plain")];
    let (view, cx) = cx.add_window_view(|_, cx| BodyView::new(cx));
    view.update(cx, |view, cx| {
        view.set_body(Arc::new(response), cx);
        view.set_mode(BodyMode::Raw, cx);
    });
    cx.run_until_parked();
    draw_window(cx);
    (view, cx)
}

/// The painted bounds of one part of the row `index` draws (`"number"` for the gutter,
/// `"code"` for the line's text), or [`None`] when that part is not drawn.
fn part_bounds(
    view: &Entity<BodyView>,
    index: usize,
    part: &str,
    cx: &mut VisualTestContext,
) -> Option<Bounds<Pixels>> {
    let base = ElementId::from(("body-view", view.entity_id()));
    let line = child_id(&base, format!("line-{index}"));
    cx.debug_bounds(leak(child_id(&line, part).to_string()))
}

/// The row `index` draws in, as [`super::render_line_with_matches`] boxes it.
fn row_bounds(view: &Entity<BodyView>, index: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let base = ElementId::from(("body-view", view.entity_id()));
    cx.debug_bounds(leak(child_id(&base, format!("line-{index}")).to_string()))
        .expect("the line is painted")
}

#[gpui::test]
fn test_every_body_line_draws_its_number_in_a_gutter_beside_the_code(cx: &mut TestAppContext) {
    // Twelve lines: past one digit, so the gutter has to pad to a shared width.
    let (view, cx) = raw_body_view(12, cx);
    let line_count = view.read_with(cx, |view, _| view.shown_lines().len());
    assert_eq!(line_count, 12, "the body has twelve lines to number");

    let mut number_widths = Vec::new();
    for index in 0..line_count {
        let row = row_bounds(&view, index, cx);
        let number = part_bounds(&view, index, "number", cx)
            .unwrap_or_else(|| panic!("line {} draws its number", index + 1));
        let code = part_bounds(&view, index, "code", cx)
            .unwrap_or_else(|| panic!("line {} draws its code", index + 1));
        assert!(
            number.right() <= code.left(),
            "line {} keeps its number left of the code: number {number:?}, code {code:?}",
            index + 1
        );
        assert!(
            number.left() >= row.left(),
            "line {} keeps its number inside its row: number {number:?}, row {row:?}",
            index + 1
        );
        assert!(
            (number.center().y - code.center().y).abs() < gpui::px(1.),
            "line {} puts its number on its own row: number {number:?}, code {code:?}",
            index + 1
        );
        number_widths.push(number.size.width);
    }

    let first = number_widths.first().copied().unwrap_or_default();
    assert!(
        number_widths.iter().all(|width| *width == first),
        "every row's number takes the same width, so the code lines up: {number_widths:?}"
    );
}

#[test]
fn test_the_gutter_numbers_the_lines_from_one() {
    assert_eq!(super::gutter_number(0, 5).as_ref(), "1");
    assert_eq!(super::gutter_number(3, 5).as_ref(), "4");
    assert_eq!(super::gutter_number(0, 12).as_ref(), " 1");
    assert_eq!(super::gutter_number(9, 12).as_ref(), "10");
    assert_eq!(super::gutter_number(11, 12).as_ref(), "12");
}

/// The row selector is built at run time, and `debug_bounds` keys on `'static` strings —
/// the test-only leak the `body_selection` tests use for the same shape of selector.
fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}
