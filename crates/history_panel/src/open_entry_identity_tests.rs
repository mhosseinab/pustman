//! A history entry opens as its own tab: opening it again, or beside the request it
//! recorded, never gives two tabs one identity — the open-tab store keys tabs by it.

use api_model::{Auth, Body, HttpRequest, ItemId, Method};
use gpui::TestAppContext;
use serde_json::json;
use storage::HistoryEntry;
use time::OffsetDateTime;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;

use super::open_entry;

fn entry(request: &HttpRequest) -> HistoryEntry {
    HistoryEntry {
        id: None,
        workspace_id: String::from("workspace-under-test"),
        created_at: OffsetDateTime::now_utc(),
        protocol: String::from("http"),
        method: String::from("GET"),
        target: request.url.clone(),
        status: Some(200),
        duration_ms: Some(1),
        request_json: serde_json::to_value(request).expect("the request serializes"),
        response_summary_json: Some(json!({ "status": 200, "timings": {}, "headers": [] })),
        response_body_prefix: None,
    }
}

#[gpui::test]
fn test_opening_one_entry_twice_gives_each_tab_its_own_identity(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    draw_window(cx);

    let request = HttpRequest {
        id: ItemId::new(),
        name: String::from("List users"),
        method: Method::Get,
        url: String::from("https://api.example.test/users"),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::None,
        settings: None,
        examples: Vec::new(),
        extra: Default::default(),
    };
    let entry = entry(&request);

    let mut ids = Vec::new();
    for _ in 0..2 {
        cx.update(|window, cx| open_entry(&entry, &workspace, window, cx));
        cx.run_until_parked();
        let id = cx.read(|cx| {
            workspace.read_with(cx, |workspace, cx| {
                workspace
                    .center_pane()
                    .read(cx)
                    .active_item()
                    .and_then(|item| item.persistent_id(cx))
                    .map(|id| id.to_string())
            })
        });
        ids.push(id.expect("the opened tab has an identity"));
    }

    assert_eq!(
        cx.read(|cx| workspace.read(cx).center_pane().read(cx).item_ids().len()),
        2
    );
    assert_ne!(ids[0], ids[1], "the two tabs do not share an identity");
    assert!(
        ids.iter().all(|id| *id != request.id.to_string()),
        "neither tab takes the recorded request's identity, which its own tab may hold"
    );
}
