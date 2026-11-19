//! The builders the body encoding tests share (`body_tests.rs`, `body_encoding_tests.rs`).
//!
//! Nothing here runs as a test: it holds no assertions and never touches the network.
//! A later step needing more helpers adds a NEW file (D9) instead of editing this one.

use std::collections::BTreeMap;

use api_model::{Auth, Body, FormField, HttpRequest, ItemId, KeyValue, Method};
use secrets::MemorySecretStore;
use url::Url;
use variables::{DynamicValues, ScopeStack};

use crate::pipeline::PipelineError;
use crate::prepare::{Prepared, prepare};
use crate::request::Header;

/// One POST to `url` carrying `body` and these header rows.
pub fn request(url: &Url, body: Body, headers: Vec<KeyValue>) -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "body test".to_string(),
        method: Method::Post,
        url: url.to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers,
        body,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// One enabled header row.
pub fn entry(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: Some(value.to_string()),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// One enabled text `form-data` field with no content type of its own.
pub fn text_field(key: &str, value: &str) -> FormField {
    FormField {
        key: key.to_string(),
        value: Some(value.to_string()),
        file: None,
        content_type: None,
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// One enabled file `form-data` field with no content type of its own. A test that needs
/// either changes it through struct update syntax.
pub fn file_field(key: &str, file: &str) -> FormField {
    FormField {
        key: key.to_string(),
        value: None,
        file: Some(file.to_string()),
        content_type: None,
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// [`prepare`] over `request` with no variables, no ancestor auths and an empty keychain:
/// what a body test needs, since none of its fields carry `{{variables}}` or secrets.
pub fn prepare_request(request: &HttpRequest) -> Result<Prepared, PipelineError> {
    let scopes = ScopeStack::new();
    let secrets = MemorySecretStore::new();
    let mut dynamic = DynamicValues::new();
    prepare(request, &[], &scopes, &secrets, &|_, _| None, &mut dynamic)
}

/// The first header of this name, compared ASCII-case-insensitively as header names are
/// (RFC 9110 §5.1).
pub fn header_named<'a>(headers: &'a [Header], name: &str) -> Option<&'a Header> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
}
