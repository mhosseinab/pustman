//! Postman v2.1 GraphQL requests: a request item whose `body.mode` is `graphql` maps to
//! [`GraphQlRequest`] and back (PRD §6.4, REQ-IO-01/03/06).
//!
//! The item is untrusted data read by the bounded Postman import: scripts are kept verbatim and
//! never executed, and every field this crate does not model is preserved so the export
//! writes the item back unchanged.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::postman::{
    ImportReport, export_auth, export_key_values, import_auth, import_events, import_headers,
    import_url,
};
use crate::{Auth, GraphQlRequest, ItemId};

/// The `extra` key holding the Postman fields that live below the item level (request, body,
/// `graphql` object) or that share a name with a modeled field, so they can be put back where
/// they came from on export.
const PRESERVED: &str = "postman_graphql";

/// Item keys that would collide with a [`GraphQlRequest`] field (or its `type` tag) if kept
/// at the top of `extra`.
const MODELED_FIELDS: &[&str] = &[
    "id",
    "type",
    "url",
    "headers",
    "auth",
    "query",
    "variables",
    "operation_name",
    "settings",
    PRESERVED,
];

/// GraphQL is sent as POST (REQ-GQL-01); any other method is kept only for the export.
const METHOD: &str = "POST";

/// The GraphQL request a Postman item describes, or `None` when its body is not
/// `mode: "graphql"` or a GraphQL field has a shape the model cannot hold without loss (the
/// HTTP import then keeps the body verbatim).
pub(crate) fn from_postman(
    item: &Map<String, Value>,
    path: &str,
    report: &mut ImportReport,
) -> Option<GraphQlRequest> {
    let request = item.get("request")?.as_object()?;
    let body = request.get("body")?.as_object()?;
    if body.get("mode").and_then(Value::as_str) != Some("graphql") {
        return None;
    }
    let graphql = match body.get("graphql") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(graphql)) => graphql.clone(),
        Some(_) => return None,
    };
    let query = text(graphql.get("query"))?.unwrap_or_default();
    // Postman stores variables as the editor's text: kept as typed, invalid JSON included.
    let variables = text(graphql.get("variables"))?.unwrap_or_default();
    let operation_name = text(graphql.get("operationName"))?;

    let request_path = format!("{path}.request");
    let mut preserved = Map::new();
    let unknown_graphql = without(&graphql, &["query", "variables", "operationName"]);
    if !unknown_graphql.is_empty() {
        preserved.insert("graphql".to_owned(), Value::Object(unknown_graphql));
    }
    let unknown_body = without(body, &["mode", "graphql"]);
    if !unknown_body.is_empty() {
        preserved.insert("body".to_owned(), Value::Object(unknown_body));
    }
    let mut unknown_request = without(request, &["method", "url", "header", "body", "auth"]);
    match request.get("method") {
        Some(Value::String(method)) if method == METHOD => {}
        None => {}
        Some(method) => {
            report.note(
                format!("{request_path}.method"),
                "a GraphQL request is sent as POST; the original method is kept for export",
            );
            unknown_request.insert("method".to_owned(), method.clone());
        }
    }
    if !unknown_request.is_empty() {
        preserved.insert("request".to_owned(), Value::Object(unknown_request));
    }

    let mut extra = BTreeMap::new();
    import_events(item, path, &mut extra, report);
    let mut colliding = Map::new();
    for (key, value) in item {
        if matches!(key.as_str(), "name" | "request" | "event") {
            continue;
        }
        if MODELED_FIELDS.contains(&key.as_str()) {
            colliding.insert(key.clone(), value.clone());
        } else {
            extra.insert(key.clone(), value.clone());
        }
    }
    if !colliding.is_empty() {
        preserved.insert("item".to_owned(), Value::Object(colliding));
    }
    if !preserved.is_empty() {
        extra.insert(PRESERVED.to_owned(), Value::Object(preserved));
    }

    Some(GraphQlRequest {
        id: ItemId::new(),
        name: crate::string_field(item, "name").unwrap_or_else(|| "Request".to_owned()),
        url: import_url(request.get("url"), &format!("{request_path}.url"), report),
        headers: import_headers(request.get("header")),
        auth: match request.get("auth") {
            Some(auth) => import_auth(auth, &format!("{request_path}.auth"), report),
            None => Auth::Inherit,
        },
        query,
        variables,
        operation_name,
        settings: None,
        extra,
    })
}

/// The Postman v2.1 item for `request`: `body.mode = "graphql"`, with everything the import
/// preserved put back at the level it came from.
pub(crate) fn to_postman(request: &GraphQlRequest) -> Value {
    let preserved = request
        .extra
        .get(PRESERVED)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let preserved_part = |name: &str| {
        preserved
            .get(name)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    };

    let mut graphql = Map::new();
    graphql.insert("query".to_owned(), Value::String(request.query.clone()));
    graphql.insert(
        "variables".to_owned(),
        Value::String(request.variables.clone()),
    );
    if let Some(operation_name) = &request.operation_name {
        graphql.insert(
            "operationName".to_owned(),
            Value::String(operation_name.clone()),
        );
    }
    merge_absent(&mut graphql, preserved_part("graphql"));

    let mut body = Map::new();
    body.insert("mode".to_owned(), Value::String("graphql".to_owned()));
    body.insert("graphql".to_owned(), Value::Object(graphql));
    merge_absent(&mut body, preserved_part("body"));

    let mut postman_request = Map::new();
    let mut unknown_request = preserved_part("request");
    let method = unknown_request
        .remove("method")
        .unwrap_or_else(|| Value::String(METHOD.to_owned()));
    postman_request.insert("method".to_owned(), method);
    postman_request.insert("url".to_owned(), Value::String(request.url.clone()));
    if !request.headers.is_empty() {
        postman_request.insert("header".to_owned(), export_key_values(&request.headers));
    }
    postman_request.insert("body".to_owned(), Value::Object(body));
    if !Auth::is_inherit(&request.auth) {
        postman_request.insert("auth".to_owned(), export_auth(&request.auth));
    }
    merge_absent(&mut postman_request, unknown_request);

    let mut item = Map::new();
    item.insert("name".to_owned(), Value::String(request.name.clone()));
    item.insert("request".to_owned(), Value::Object(postman_request));
    for (key, value) in &request.extra {
        if key != PRESERVED {
            item.insert(key.clone(), value.clone());
        }
    }
    merge_absent(&mut item, preserved_part("item"));
    Value::Object(item)
}

/// A string field as text: `Some(None)` when absent or null, `None` (refuse the mapping) when
/// it holds anything but a string.
fn text(value: Option<&Value>) -> Option<Option<String>> {
    match value {
        None | Some(Value::Null) => Some(None),
        Some(Value::String(text)) => Some(Some(text.clone())),
        Some(_) => None,
    }
}

fn without(object: &Map<String, Value>, modeled: &[&str]) -> Map<String, Value> {
    object
        .iter()
        .filter(|(key, _)| !modeled.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Add preserved fields without overwriting the modeled ones, which carry the user's edits.
fn merge_absent(target: &mut Map<String, Value>, preserved: Map<String, Value>) {
    for (key, value) in preserved {
        target.entry(key).or_insert(value);
    }
}

#[cfg(test)]
#[path = "postman_graphql_tests.rs"]
mod tests;
