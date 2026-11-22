//! The push-shaped draft replacement's re-seed of the disabled auto headers
//! (REQ-HTTP-03, #170): `Headers::draft_changed` is the path a replaced draft takes
//! into a tab that stays open — the request editor pushes every draft edit back
//! through it — and it re-seeds the tab's switches from the pushed draft, so a tab
//! never keeps the switches of the request it replaced. The cURL import's replacement
//! cannot pin that re-seed: `replace_request` rebuilds the tab through
//! [`Headers::new`], which seeds its own way and stays green without `draft_changed`
//! re-seeding anything (S27d review 2).

use std::collections::BTreeMap;

use api_model::{Body, HttpRequest, Method, RawLanguage, RequestSettings};
use gpui::{Entity, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use super::Headers;

/// A draft POST with a JSON body — a request whose auto headers (User-Agent among
/// them) the tab lists with a send switch each — that leaves `names` out of the send.
fn request_disabling(names: &[&str]) -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.method = Method::Post;
    request.url = String::from("https://example.com/items");
    request.body = Body::Raw {
        language: RawLanguage::Json,
        text: String::from("{\"name\":\"pustman\"}"),
        extra: BTreeMap::new(),
    };
    request.settings = Some(RequestSettings {
        disabled_auto_headers: names.iter().copied().map(String::from).collect(),
        ..RequestSettings::default()
    });
    request
}

/// The Headers tab on the app's real init order, opened on `request`.
fn open_headers(
    cx: &mut TestAppContext,
    request: HttpRequest,
) -> (Entity<Headers>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (headers, cx) = cx.add_window_view(|window, cx| Headers::new(&request, window, cx));
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (headers, cx)
}

/// Whether the tab's switches leave `name` out of the send — the state its send
/// switch renders (`Headers::draft_settings`).
fn tab_disables(headers: &Entity<Headers>, cx: &VisualTestContext, name: &str) -> bool {
    cx.read(|cx| {
        headers.read_with(cx, |headers, _| {
            headers.draft_settings().is_auto_header_disabled(name)
        })
    })
}

/// Replacing the draft by pushing it into the tab re-seeds the switches from the
/// pushed draft: the replaced request's stale switch is gone and the replacement's
/// own is on (`Headers::draft_changed`, the push every draft edit takes).
#[gpui::test]
fn test_pushing_a_replaced_draft_reseeds_the_disabled_auto_headers(cx: &mut TestAppContext) {
    // The tab opens on a draft that leaves User-Agent out of the send.
    let (headers, cx) = open_headers(cx, request_disabling(&["User-Agent"]));
    assert!(
        cx.debug_bounds("auto-toggle-User-Agent").is_some(),
        "the tab lists the draft's auto rows with a send switch each"
    );
    assert!(
        tab_disables(&headers, cx, "User-Agent"),
        "the tab seeds its switches from the request it opened (REQ-HTTP-03)"
    );

    // Replace the draft wholesale and push it back into the same tab — the shape
    // `RequestEditor` pushes every draft edit with — with a request that leaves
    // Content-Type out instead.
    let replacement = request_disabling(&["Content-Type"]);
    cx.update(|_window, cx| {
        headers.update(cx, |headers, cx| headers.draft_changed(&replacement, cx));
    });
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        !tab_disables(&headers, cx, "User-Agent"),
        "the replaced draft's switch does not ride along: the pushed draft names it no more"
    );
    assert!(
        tab_disables(&headers, cx, "Content-Type"),
        "and the pushed draft's own switch arrives (REQ-HTTP-03)"
    );
}
