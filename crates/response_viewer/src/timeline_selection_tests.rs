//! The Timeline's labels and values are text the user copies out: a double click takes a
//! word, a triple click the row as `label: value`, and select-all then copy takes the whole
//! tab — phases and network facts as `label: value` lines, hops as tab-separated rows.

use std::time::Duration;

use api_model::Method;
use gpui::{
    ElementId, Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext, px,
};
use http_engine::{Header, RedirectHop, Response, ResponseBody, Timings};
use ui::test_support::{draw_window, init_test};
use url::Url;

use super::TimelineTab;
use crate::child_id;

fn timeline(cx: &mut TestAppContext) -> (Entity<TimelineTab>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(Vec::new()));
    response.timings = Timings {
        total: Some(Duration::from_millis(200)),
        ..Timings::default()
    };
    response.redirects = vec![RedirectHop {
        method: Method::Get,
        url: Url::parse("https://example.test/start").expect("the fixture URL must parse"),
        status: 301,
        headers: vec![Header::new("content-type", "application/json")],
        location: Some(
            Url::parse("https://example.test/second").expect("the fixture Location must parse"),
        ),
        timing: Some(Duration::from_millis(12)),
    }];
    let (view, cx) = cx.add_window_view(|_, cx| TimelineTab::new(cx));
    view.update(cx, |view, cx| view.set_response(&response, cx));
    draw_window(cx);
    (view, cx)
}

fn cell_bounds(
    view: &Entity<TimelineTab>,
    row_key: &str,
    column: &str,
    cx: &mut VisualTestContext,
) -> gpui::Bounds<Pixels> {
    let base = ElementId::from(("timeline-tab", view.entity_id()));
    let row = child_id(&base, row_key.to_owned());
    let selector = child_id(&row, column.to_owned()).to_string();
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
        .expect("the cell is painted")
}

fn click(at: Point<Pixels>, click_count: usize, cx: &mut VisualTestContext) {
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

fn clicks(at: Point<Pixels>, count: usize, cx: &mut VisualTestContext) {
    for click_count in 1..=count {
        click(at, click_count, cx);
    }
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

fn copy_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    }
}

fn select_all_keystroke() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    }
}

#[gpui::test]
fn test_a_double_click_on_a_hop_url_copies_the_word_under_the_pointer(cx: &mut TestAppContext) {
    let (view, cx) = timeline(cx);
    let url = cell_bounds(&view, "hop-row-0", "url", cx);
    clicks(Point::new(url.left() + px(2.), url.center().y), 2, cx);
    cx.simulate_keystrokes(copy_keystroke());

    assert_eq!(clipboard(cx).as_deref(), Some("https"));
}

#[gpui::test]
fn test_a_triple_click_on_a_network_value_copies_the_row_as_label_colon_value(
    cx: &mut TestAppContext,
) {
    let (view, cx) = timeline(cx);
    let value = cell_bounds(&view, "network-HTTP version", "value", cx);
    clicks(Point::new(value.left() + px(2.), value.center().y), 3, cx);
    cx.simulate_keystrokes(copy_keystroke());

    assert_eq!(clipboard(cx).as_deref(), Some("HTTP version: HTTP/1.1\n"));
}

#[gpui::test]
fn test_select_all_then_copy_takes_phases_network_facts_and_hops(cx: &mut TestAppContext) {
    let (view, cx) = timeline(cx);
    let value = cell_bounds(&view, "network-HTTP version", "value", cx);
    click(Point::new(value.left() + px(2.), value.center().y), 1, cx);
    cx.simulate_keystrokes(select_all_keystroke());
    cx.simulate_keystrokes(copy_keystroke());

    let copied = clipboard(cx).unwrap_or_default();
    assert!(copied.contains("Total: 200 ms\n"), "phases: {copied:?}");
    assert!(
        copied.contains("HTTP version: HTTP/1.1\n"),
        "network: {copied:?}"
    );
    assert!(
        copied
            .ends_with("GET\thttps://example.test/start\t301\thttps://example.test/second\t12 ms"),
        "the hop row, tab-separated: {copied:?}"
    );
}
