//! The console wire (S32w): the app's init order must install the `ConsoleStore` global,
//! register the Console panel in every workspace window's bottom dock, and record every
//! cancellation the app's one HTTP client reports (REQ-CON-01, REQ-HTTP-12).

use api_model::Method;
use console::{ConsoleLevel, ConsolePanel, ConsoleStore};
use gpui::TestAppContext;
use http_engine::{RequestBody, ResolvedRequest, SendSettings};
use pustman::StoreLocation;
use request_editor::send::RequestClient;
use ui::test_support::draw_window;
use workspace::Workspace;

#[gpui::test]
fn wire_console_panel_registered_in_bottom_dock(cx: &mut TestAppContext) {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::InMemory));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| pustman::StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    // `console::init` registers the panel as the workspace is created; the store opens on
    // a background task and must land before anything is recorded.
    cx.run_until_parked();
    draw_window(cx);

    // The bottom dock holds the Console panel, whose stable id its `Panel` impl reports
    // (S32d). `console`'s `PANEL_ID` is module-private, so the panel is found by type and
    // its id asserted against the value `Panel::panel_id` returns.
    let console_panel_ids: Vec<&'static str> = cx.read(|cx| {
        workspace.read_with(cx, |workspace, cx| {
            workspace
                .bottom_dock()
                .read(cx)
                .panels()
                .iter()
                .filter(|panel| panel.to_any_view().downcast::<ConsolePanel>().is_ok())
                .map(|panel| panel.panel_id(cx))
                .collect()
        })
    });
    assert_eq!(
        console_panel_ids,
        vec!["console"],
        "the bottom dock holds the Console panel once the workspace exists (REQ-CON-01)"
    );

    assert!(
        cx.read(|cx| cx.has_global::<ConsoleStore>()),
        "console::init installs the ConsoleStore global the panel reads through"
    );

    // A cancel on the app's own client lands one entry (REQ-HTTP-12). The send is
    // cancelled before the test scheduler runs its work, so the transport is never
    // called: `cancel` drops the in-flight task and the feed records "Cancelled".
    let client = cx.read(|cx| cx.global::<RequestClient>().0.clone());
    let request = ResolvedRequest::new(
        Method::Get,
        "http://example.test/never-sent",
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("the fixture request URL parses");
    let _in_flight = client.update(cx, |client, cx| client.send(request, cx));
    client.update(cx, |client, cx| client.cancel(cx));
    cx.run_until_parked();

    cx.read(|cx| {
        let entries = cx.global::<ConsoleStore>().entries();
        assert_eq!(
            entries.len(),
            1,
            "a cancelled send lands exactly one Console entry"
        );
        let entry = entries.front().expect("the one entry just recorded");
        assert_eq!(entry.level, ConsoleLevel::Warning);
        assert_eq!(entry.raw_response, "Cancelled");
    });
}
