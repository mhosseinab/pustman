//! The editor keeps the URL text and the Params table in step (`request_editor::params`):
//! every enabled query row is already in the URL. The send must carry each param once,
//! not once from the URL and again from the table.

use std::collections::BTreeMap;

use api_model::{Auth, Body, HttpRequest, ItemId, KeyValue, Method};
use variables::{DynamicValues, ScopeStack};

use crate::pipeline::resolve_request;

#[test]
fn test_a_param_the_url_already_carries_is_sent_once() {
    let request = request(
        "https://example.test/search?q=test",
        vec![entry("q", "test")],
    );

    let (resolved, _) =
        resolve_request(&request, &ScopeStack::new(), &mut DynamicValues::new()).unwrap();

    assert_eq!(resolved.url.as_str(), "https://example.test/search?q=test");
}

#[test]
fn test_repeated_keys_mirrored_in_the_url_are_each_sent_once() {
    let request = request(
        "https://example.test/search?tag=a&tag=b",
        vec![entry("tag", "a"), entry("tag", "b")],
    );

    let (resolved, _) =
        resolve_request(&request, &ScopeStack::new(), &mut DynamicValues::new()).unwrap();

    assert_eq!(
        resolved.url.as_str(),
        "https://example.test/search?tag=a&tag=b"
    );
}

#[test]
fn test_a_param_missing_from_the_url_is_still_appended() {
    let request = request(
        "https://example.test/search?q=test",
        vec![entry("q", "test"), entry("page", "2")],
    );

    let (resolved, _) =
        resolve_request(&request, &ScopeStack::new(), &mut DynamicValues::new()).unwrap();

    assert_eq!(
        resolved.url.as_str(),
        "https://example.test/search?q=test&page=2"
    );
}

fn request(url: &str, params: Vec<KeyValue>) -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "mirror test".to_string(),
        method: Method::Get,
        url: url.to_string(),
        params,
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

fn entry(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(value.to_string()),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}
