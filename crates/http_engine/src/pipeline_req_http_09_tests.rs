//! The two REQ-HTTP-09 overrides that had no typed [`RequestSettings`] field — "keep
//! `Authorization` across redirects" and "disable cookie jar" — reach what the
//! transport executes: the resolver merges them into the resolved request's
//! [`SendSettings`], and a request that overrides neither keeps the defaults.

use std::collections::BTreeMap;

use api_model::{Auth, Body, HttpRequest, ItemId, Method, RequestSettings};
use variables::{DynamicValues, ScopeStack};

use crate::pipeline::resolve_request;

/// One GET whose settings block is `settings`.
fn request(settings: Option<RequestSettings>) -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "pipeline test".to_string(),
        method: Method::Get,
        url: "https://example.test/x".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::Inherit,
        settings,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

#[test]
fn keep_authorization_and_disable_cookie_jar_overrides_reach_the_send() {
    let pinned = request(Some(RequestSettings {
        keep_authorization: Some(true),
        disable_cookie_jar: Some(true),
        ..Default::default()
    }));
    let (resolved, _) = resolve_request(&pinned, &ScopeStack::new(), &mut DynamicValues::new())
        .expect("the request resolves");
    assert!(
        resolved.settings.keep_authorization,
        "keep_authorization must reach the send"
    );
    assert!(
        resolved.settings.disable_cookie_jar,
        "disable_cookie_jar must reach the send"
    );

    // A request that overrides neither inherits the defaults: Authorization is
    // dropped across a redirect and the cookie jar applies.
    let (inherited, _) = resolve_request(
        &request(None),
        &ScopeStack::new(),
        &mut DynamicValues::new(),
    )
    .expect("the request resolves");
    assert!(
        !inherited.settings.keep_authorization,
        "a request that overrides nothing must not keep Authorization across redirects"
    );
    assert!(
        !inherited.settings.disable_cookie_jar,
        "a request that overrides nothing must use the cookie jar"
    );
}
