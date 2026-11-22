//! Cookie cells are text the user copies out: a double click takes a word, a triple
//! click the row with its columns tab-separated, and select-all then copy takes the table
//! (one row per line, so it pastes into a spreadsheet).

use gpui::{
    ElementId, Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext, px,
};
use http_engine::Header;
use ui::test_support::{draw_window, init_test};

use super::CookiesTab;
use crate::child_id;

fn cookies_tab(cx: &mut TestAppContext) -> (Entity<CookiesTab>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let (view, cx) = cx.add_window_view(|_, cx| CookiesTab::new(cx));
    let headers = vec![
        Header::new("Set-Cookie", "sid=token-REDACTED-example; Path=/"),
        Header::new("Set-Cookie", "theme=dark; Path=/app"),
    ];
    view.update(cx, |view, cx| view.set_headers(&headers, cx));
    draw_window(cx);
    (view, cx)
}

fn cell_bounds(
    view: &Entity<CookiesTab>,
    row_key: &str,
    column: &str,
    cx: &mut VisualTestContext,
) -> gpui::Bounds<Pixels> {
    let base = ElementId::from(("cookies-tab", view.entity_id()));
    let row = child_id(&base, row_key.to_owned());
    let selector = child_id(&row, column.to_owned()).to_string();
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
        .expect("the cell is painted")
}

fn clicks(at: Point<Pixels>, count: usize, cx: &mut VisualTestContext) {
    for click_count in 1..=count {
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        draw_window(cx);
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
        });
        draw_window(cx);
    }
}

fn copied(cx: &mut VisualTestContext) -> String {
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    });
    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default()
}

#[gpui::test]
fn test_a_double_click_on_a_cookie_value_copies_the_word_under_the_pointer(
    cx: &mut TestAppContext,
) {
    let (view, cx) = cookies_tab(cx);
    let value = cell_bounds(&view, "theme#0", "value", cx);
    clicks(Point::new(value.left() + px(2.), value.center().y), 2, cx);

    assert_eq!(copied(cx), "dark");
}

#[gpui::test]
fn test_a_triple_click_copies_the_cookie_row_tab_separated(cx: &mut TestAppContext) {
    let (view, cx) = cookies_tab(cx);
    let name = cell_bounds(&view, "theme#0", "name", cx);
    clicks(Point::new(name.left() + px(2.), name.center().y), 3, cx);
    let row = copied(cx);

    assert!(row.starts_with("theme\tdark\t"), "got {row:?}");
    assert_eq!(row.matches('\t').count(), 8, "nine columns: {row:?}");
    assert!(
        !row.trim_end_matches('\n').contains('\n'),
        "one row: {row:?}"
    );
}

#[gpui::test]
fn test_select_all_then_copy_takes_every_cookie_row(cx: &mut TestAppContext) {
    let (view, cx) = cookies_tab(cx);
    let name = cell_bounds(&view, "sid#0", "name", cx);
    clicks(Point::new(name.left() + px(2.), name.center().y), 1, cx);
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    let table = copied(cx);
    let lines: Vec<&str> = table.lines().collect();

    assert_eq!(lines.len(), 2, "got {table:?}");
    assert!(lines[0].starts_with("sid\ttoken-REDACTED-example\t"));
    assert!(lines[1].starts_with("theme\tdark\t"));
}
