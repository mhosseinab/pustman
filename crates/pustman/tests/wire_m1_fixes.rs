//! PROBE (S33o): reproduce the "Response → Timeline shifts the whole window" bug in the
//! full workspace context and print the painted bounds before and after.

use std::time::Duration;

use api_model::Method;
use gpui::{Entity, Modifiers, TestAppContext};
use http_engine::{Header, RedirectHop, Response, ResponseBody, Timings};
use pustman::StoreLocation;
use pustman_actions::workspace::NewRequest;
use request_editor::RequestEditor;
use ui::test_support::draw_window;
use url::Url;
use workspace::Workspace;

#[gpui::test]
fn probe_timeline_shift(cx: &mut TestAppContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| pustman::StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();

    cx.dispatch_action(NewRequest);
    cx.run_until_parked();
    draw_window(cx);

    let editor: Entity<RequestEditor> = cx
        .read(|cx| {
            workspace.read_with(cx, |workspace, cx| {
                workspace
                    .center_pane()
                    .read(cx)
                    .active_item()
                    .and_then(|item| item.to_any_view().downcast::<RequestEditor>().ok())
            })
        })
        .expect("NewRequest opens a request editor");

    editor.update(cx, |editor, cx| {
        let response_view = editor.response_view().clone();
        response_view.update(cx, |view, cx| view.set_response(long_response(), cx));
    });
    draw_window(cx);

    let tab_id = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace.center_pane().read(cx).item_ids()[0]
        })
    });
    let tab_selector: &'static str = format!("pane-tab-{tab_id}").leak();
    let before = cx.debug_bounds(tab_selector).expect("the tab strip is drawn");
    let title_before = cx.debug_bounds("title-bar");
    eprintln!("PROBE pane tab before = {before:?}");
    eprintln!("PROBE title bar before = {title_before:?}");
    eprintln!("PROBE note before = {:?}", cx.debug_bounds("timeline-unmeasured-note"));

    let timeline_tab_selector: &'static str =
        gpui::ElementId::from(("response-tab", 3usize)).to_string().leak();
    let timeline_tab = cx
        .debug_bounds(timeline_tab_selector)
        .expect("the Timeline tab is drawn");
    eprintln!("PROBE timeline tab bounds = {timeline_tab:?}");
    cx.simulate_click(timeline_tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);

    let after = cx.debug_bounds(tab_selector).expect("the tab strip is drawn");
    let title_after = cx.debug_bounds("title-bar");
    eprintln!("PROBE pane tab after = {after:?}");
    eprintln!("PROBE title bar after = {title_after:?}");
    eprintln!("PROBE note after = {:?}", cx.debug_bounds("timeline-unmeasured-note"));

    assert_eq!(
        before.origin, after.origin,
        "selecting the Timeline tab must not move the pane tab strip"
    );
}

/// A response whose timeline is taller than the response pane: every measured phase, the
/// network facts and a long redirect chain.
fn long_response() -> Response {
    let mut response = Response::new(200, "OK", ResponseBody::Bytes(Vec::new()));
    response.timings = Timings {
        prepare: Some(Duration::from_millis(5)),
        dns: Some(Duration::from_millis(12)),
        connect: Some(Duration::from_millis(30)),
        tls: Some(Duration::from_millis(45)),
        first_byte: Some(Duration::from_millis(120)),
        download: Some(Duration::from_millis(80)),
        total: Some(Duration::from_millis(292)),
        ..Timings::default()
    };
    response.redirects = (0..30)
        .map(|hop_index| RedirectHop {
            method: Method::Get,
            url: Url::parse(&format!("https://example.test/hop/{hop_index}"))
                .expect("the fixture hop URL parses"),
            status: 302,
            headers: vec![Header::new("location", "https://example.test/next")],
            location: Some(
                Url::parse("https://example.test/next").expect("the fixture Location parses"),
            ),
            timing: Some(Duration::from_millis(10)),
        })
        .collect();
    response
}
