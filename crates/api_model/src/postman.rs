//! Postman collection v2.1 / v2.0 import and v2.1 export (PRD §6.4, REQ-IO-01/02/03/06).
//!
//! Imported files are **untrusted data** (AGENTS.md › Security): sizes and nesting depth are
//! bounded *before* full parsing, imported scripts are preserved verbatim and **never executed**
//! (plan D3/D4/D5 — there is no scripting runtime), and a secret-typed variable never enters the
//! model at all. Everything the step does not model is preserved verbatim so an export never
//! drops it.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
    ApiKeyLocation, Auth, Body, Collection, Environment, Example, ExampleResponse, Folder,
    FormField, HttpRequest, ItemId, KeyValue, Method, RawLanguage, Request, Variable, VariableKind,
};

/// What an import accepts. Both bounds are enforced **before** full parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportLimits {
    /// Maximum accepted input size in bytes.
    pub max_bytes: usize,
    /// Maximum accepted JSON nesting depth.
    pub max_depth: usize,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_bytes: 50 * 1024 * 1024,
            max_depth: 64,
        }
    }
}

/// Why an import refused its input.
#[derive(Debug, PartialEq, Eq)]
pub enum ImportError {
    /// The input is larger than [`ImportLimits::max_bytes`].
    TooLarge { size: usize, limit: usize },
    /// The input nests deeper than [`ImportLimits::max_depth`].
    TooDeep { limit: usize },
    /// The input is not the JSON the format promises.
    Invalid(String),
    /// A field the format requires is absent.
    MissingField(&'static str),
}

/// A value the caller must put in the OS keychain. Keyed by the Postman variable name; the
/// caller maps it to a [`crate::SecretKey`] when it knows the workspace and scope.
pub type SecretValuesToStore = Vec<(String, String)>;

/// One thing the import could not map onto the model but preserved rather than dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportProblem {
    /// Where it came from, e.g. `item[0].item[2].request`.
    pub path: String,
    /// What was preserved and why.
    pub detail: String,
}

/// Everything an import preserved or could not map. Scripts and unsupported auth kinds are
/// listed here as "preserved, not executed".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub problems: Vec<ImportProblem>,
}

impl ImportReport {
    fn note(&mut self, path: impl Into<String>, detail: impl Into<String>) {
        self.problems.push(ImportProblem {
            path: path.into(),
            detail: detail.into(),
        });
    }
}

/// What an export dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportReport {
    pub warnings: Vec<String>,
}

/// Import a Postman collection (v2.1 or v2.0, detected by its `info.schema` URL).
pub fn import_collection(
    bytes: &[u8],
    limits: &ImportLimits,
) -> Result<(Collection, ImportReport), ImportError> {
    let value = parse_bounded(bytes, limits)?;
    let mut report = ImportReport::default();
    let root = as_object(&value, "collection")?;

    let info = as_object(root.get("info").unwrap_or(&Value::Null), "info")?;
    let name = string_field(info, "name").unwrap_or_else(|| "Imported collection".to_owned());
    let schema = string_field(info, "schema").unwrap_or_default();
    let version = if schema.contains("/v2.0.0/") {
        "2.0"
    } else {
        "2.1"
    };
    if !schema.contains("/v2.1.0/") && !schema.contains("/v2.0.0/") {
        report.note(
            "info.schema",
            format!("unrecognised schema URL {schema:?}; read as v2.1"),
        );
    }

    let mut collection = Collection {
        id: ItemId::new(),
        name,
        description: string_field(info, "description"),
        auth: Auth::Inherit,
        variables: Vec::new(),
        order: Vec::new(),
        children: Vec::new(),
        extra: BTreeMap::new(),
    };
    collection.extra.insert(
        "format_version".to_owned(),
        Value::String(version.to_owned()),
    );
    // Unknown top-level fields round-trip, exactly as folder and request fields do.
    for (key, value) in root {
        if matches!(
            key.as_str(),
            "info" | "item" | "auth" | "variable" | "event" | "format_version"
        ) {
            continue;
        }
        collection.extra.insert(key.clone(), value.clone());
    }

    if let Some(auth) = root.get("auth") {
        collection.auth = import_auth(auth, "auth", &mut report);
    }
    if let Some(items) = root.get("item").and_then(Value::as_array) {
        import_children(
            items,
            "item",
            &mut collection.children,
            &mut collection.order,
            &mut report,
        );
    }
    if let Some(vars) = root.get("variable").and_then(Value::as_array) {
        collection.variables = import_variables(vars, "variable", &mut report);
    }
    import_events(root, "collection", &mut collection.extra, &mut report);

    Ok((collection, report))
}

/// Import a Postman environment. `secret`-typed values are returned separately for the caller
/// to put in the keychain and are NOT part of the returned [`Environment`].
pub fn import_environment(
    bytes: &[u8],
    limits: &ImportLimits,
) -> Result<(Environment, SecretValuesToStore, ImportReport), ImportError> {
    let value = parse_bounded(bytes, limits)?;
    let mut report = ImportReport::default();
    let root = as_object(&value, "environment")?;
    let (variables, secrets) = split_secrets(root, "values", &mut report);
    let environment = Environment {
        id: ItemId::new(),
        name: string_field(root, "name").unwrap_or_else(|| "Imported environment".to_owned()),
        variables,
        extra: BTreeMap::new(),
    };
    Ok((environment, secrets, report))
}

/// Import Postman globals: the same shape as an environment, without the container.
pub fn import_globals(
    bytes: &[u8],
    limits: &ImportLimits,
) -> Result<(Vec<Variable>, SecretValuesToStore, ImportReport), ImportError> {
    let value = parse_bounded(bytes, limits)?;
    let mut report = ImportReport::default();
    let root = as_object(&value, "globals")?;
    let (variables, secrets) = split_secrets(root, "values", &mut report);
    Ok((variables, secrets, report))
}

/// Emit a Postman v2.1 collection. Non-HTTP request kinds are skipped and listed as warnings:
/// v2.1 has no request shape this crate models for them.
pub fn export_collection(collection: &Collection) -> (Vec<u8>, ExportReport) {
    let mut report = ExportReport::default();
    let mut items = Vec::new();
    export_children(&collection.children, &mut items, &mut report);

    let mut root = serde_json::Map::new();
    let mut info = serde_json::Map::new();
    info.insert("name".to_owned(), Value::String(collection.name.clone()));
    info.insert(
        "schema".to_owned(),
        Value::String(
            "https://schema.getpostman.com/json/collection/v2.1.0/collection.json".to_owned(),
        ),
    );
    if let Some(description) = &collection.description {
        info.insert("description".to_owned(), Value::String(description.clone()));
    }
    root.insert("info".to_owned(), Value::Object(info));
    root.insert("item".to_owned(), Value::Array(items));
    if !Auth::is_inherit(&collection.auth) {
        root.insert("auth".to_owned(), export_auth(&collection.auth));
    }
    let variables: Vec<Value> = collection.variables.iter().map(export_variable).collect();
    if !variables.is_empty() {
        root.insert("variable".to_owned(), Value::Array(variables));
    }
    // Unknown top-level fields round-trip too — a collection-level `event[]` script is preserved
    // verbatim on import and must not vanish on export. `format_version` is this crate's own
    // bookkeeping and is not a Postman field.
    for (key, value) in &collection.extra {
        if matches!(key.as_str(), "format_version") || root.contains_key(key) {
            continue;
        }
        root.insert(key.clone(), value.clone());
    }
    match serde_json::to_vec_pretty(&Value::Object(root)) {
        Ok(mut bytes) => {
            bytes.push(b'\n');
            (bytes, report)
        }
        Err(error) => {
            report
                .warnings
                .push(format!("the collection could not be serialized: {error}"));
            (Vec::new(), report)
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Parsing bounds (before full parsing)

fn parse_bounded(bytes: &[u8], limits: &ImportLimits) -> Result<Value, ImportError> {
    if bytes.len() > limits.max_bytes {
        return Err(ImportError::TooLarge {
            size: bytes.len(),
            limit: limits.max_bytes,
        });
    }
    if nesting_depth(bytes) > limits.max_depth {
        return Err(ImportError::TooDeep {
            limit: limits.max_depth,
        });
    }
    serde_json::from_slice(bytes).map_err(|error| ImportError::Invalid(error.to_string()))
}

/// The deepest `{`/`[` nesting of a JSON document, scanning the raw bytes so the check happens
/// before a parse can allocate for the whole tree. String contents and escapes are skipped.
fn nesting_depth(bytes: &[u8]) -> usize {
    let mut depth = 0usize;
    let mut widest = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                widest = widest.max(depth);
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    widest
}

// ---------------------------------------------------------------------------------------------
// Collection tree

fn import_children(
    items: &[Value],
    path: &str,
    children: &mut Vec<crate::Child>,
    order: &mut Vec<ItemId>,
    report: &mut ImportReport,
) {
    for (index, item) in items.iter().enumerate() {
        let at = format!("{path}[{index}]");
        let Some(object) = item.as_object() else {
            report.note(&at, "item is not an object; skipped");
            continue;
        };
        if object.contains_key("item") {
            let Some(nested) = object.get("item").and_then(Value::as_array) else {
                report.note(&at, "folder without an item array; skipped");
                continue;
            };
            let mut folder = Folder {
                id: ItemId::new(),
                name: string_field(object, "name").unwrap_or_else(|| "Folder".to_owned()),
                description: string_field(object, "description"),
                auth: Auth::Inherit,
                children: Vec::new(),
                order: Vec::new(),
                extra: BTreeMap::new(),
            };
            if let Some(auth) = object.get("auth") {
                folder.auth = import_auth(auth, &format!("{at}.auth"), report);
            }
            import_events(object, &at, &mut folder.extra, report);
            // Unknown folder fields round-trip too, exactly as request fields do.
            for (key, value) in object {
                if matches!(
                    key.as_str(),
                    "name" | "item" | "auth" | "event" | "description"
                ) {
                    continue;
                }
                folder.extra.insert(key.clone(), value.clone());
            }
            import_children(
                nested,
                &format!("{at}.item"),
                &mut folder.children,
                &mut folder.order,
                report,
            );
            order.push(folder.id);
            children.push(crate::Child::Folder(folder));
        } else if object.contains_key("request") {
            let request = import_request(object, &at, report);
            order.push(request_id(&request));
            children.push(crate::Child::Request(request));
        } else {
            report.note(&at, "item is neither a folder nor a request; skipped");
        }
    }
}

fn request_id(request: &Request) -> ItemId {
    match request {
        Request::Http(http) => http.id,
        Request::Unsupported(_) => ItemId::new(),
    }
}

fn import_request(
    item: &serde_json::Map<String, Value>,
    path: &str,
    report: &mut ImportReport,
) -> Request {
    let name = string_field(item, "name").unwrap_or_else(|| "Request".to_owned());
    let request = item.get("request").cloned().unwrap_or(Value::Null);
    let Some(request) = request.as_object() else {
        report.note(
            format!("{path}.request"),
            "request is not an object; preserved verbatim",
        );
        return Request::Unsupported(Value::Object(item.clone()));
    };

    // A Postman request without a method is not an HTTP request this crate models.
    let Some(method) = string_field(request, "method") else {
        report.note(format!("{path}.request"), "no method; preserved verbatim");
        return Request::Unsupported(Value::Object(item.clone()));
    };
    let method = method
        .parse::<Method>()
        .unwrap_or_else(|_| Method::Custom(method.clone()));

    let url = import_url(request.get("url"), &format!("{path}.request.url"), report);
    let headers = import_headers(request.get("header"));
    let body = import_body(request.get("body"), &format!("{path}.request.body"), report);
    let auth = match request.get("auth") {
        Some(auth) => import_auth(auth, &format!("{path}.request.auth"), report),
        None => Auth::Inherit,
    };

    let mut http = HttpRequest {
        id: ItemId::new(),
        name,
        method,
        url,
        params: Vec::new(),
        path_variables: Vec::new(),
        headers,
        body,
        auth,
        settings: None,
        examples: import_responses(item.get("response"), path, report),
        extra: BTreeMap::new(),
    };
    import_events(item, path, &mut http.extra, report);
    for (key, value) in item {
        if matches!(key.as_str(), "name" | "request" | "response" | "event") {
            continue;
        }
        http.extra.insert(key.clone(), value.clone());
    }
    Request::Http(http)
}

fn import_url(value: Option<&Value>, path: &str, report: &mut ImportReport) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(raw)) => raw.clone(),
        Some(Value::Object(object)) => {
            if let Some(raw) = string_field(object, "raw") {
                return raw;
            }
            // No `raw`: rebuild a host/path form so nothing is lost.
            let host = joined(object.get("host"), ".");
            let port = string_field(object, "port");
            let path_segments = joined(object.get("path"), "/");
            let mut url = host;
            if let Some(port) = port {
                url.push(':');
                url.push_str(&port);
            }
            if !path_segments.is_empty() {
                url.push('/');
                url.push_str(&path_segments);
            }
            report.note(path, "url without raw; rebuilt from host/path");
            url
        }
        Some(other) => {
            report.note(
                path,
                format!("url is {:?}; kept as its display form", other),
            );
            other.to_string()
        }
    }
}

fn joined(value: Option<&Value>, separator: &str) -> String {
    match value {
        Some(Value::String(one)) => one.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|part| match part {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join(separator),
        _ => String::new(),
    }
}

fn import_headers(value: Option<&Value>) -> Vec<KeyValue> {
    let Some(headers) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for header in headers {
        let Some(object) = header.as_object() else {
            continue;
        };
        let Some(key) = string_field(object, "key") else {
            continue;
        };
        out.push(KeyValue {
            key,
            value: string_field(object, "value"),
            enabled: !bool_field(object, "disabled").unwrap_or(false),
            description: string_field(object, "description"),
            extra: BTreeMap::new(),
        });
    }
    out
}

/// The syntax a Postman `raw` body names, or [`RawLanguage::Text`] when it names none we model.
fn raw_language(name: &str) -> RawLanguage {
    match name {
        "json" => RawLanguage::Json,
        "xml" => RawLanguage::Xml,
        "html" => RawLanguage::Html,
        "javascript" => RawLanguage::JavaScript,
        _ => RawLanguage::Text,
    }
}

fn raw_language_name(language: &RawLanguage) -> &'static str {
    match language {
        RawLanguage::Text => "text",
        RawLanguage::Json => "json",
        RawLanguage::Xml => "xml",
        RawLanguage::Html => "html",
        RawLanguage::JavaScript => "javascript",
    }
}

fn import_body(value: Option<&Value>, path: &str, report: &mut ImportReport) -> Body {
    let Some(body) = value.and_then(Value::as_object) else {
        return Body::None;
    };
    let mode = string_field(body, "mode").unwrap_or_default();
    match mode.as_str() {
        "raw" => {
            let text = string_field(body, "raw").unwrap_or_default();
            let language = body
                .get("options")
                .and_then(|options| options.get("raw"))
                .and_then(|raw| raw.get("language"))
                .and_then(Value::as_str)
                .map(raw_language)
                .unwrap_or(RawLanguage::Json);
            Body::Raw {
                language,
                text,
                extra: BTreeMap::new(),
            }
        }
        "urlencoded" => Body::UrlEncoded {
            fields: import_key_values(body.get("urlencoded")),
            extra: BTreeMap::new(),
        },
        "formdata" => Body::FormData {
            fields: import_form_fields(body.get("formdata")),
            extra: BTreeMap::new(),
        },
        "file" => {
            let file = body
                .get("file")
                .and_then(|file| file.get("src"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            Body::Binary {
                file,
                extra: BTreeMap::new(),
            }
        }
        other => {
            report.note(path, format!("body mode {other:?} preserved verbatim"));
            Body::Unsupported(Value::Object(body.clone()))
        }
    }
}

fn import_key_values(value: Option<&Value>) -> Vec<KeyValue> {
    let Some(fields) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for field in fields {
        let Some(object) = field.as_object() else {
            continue;
        };
        let Some(key) = string_field(object, "key") else {
            continue;
        };
        out.push(KeyValue {
            key,
            value: string_field(object, "value"),
            enabled: !bool_field(object, "disabled").unwrap_or(false),
            description: string_field(object, "description"),
            extra: BTreeMap::new(),
        });
    }
    out
}

fn import_form_fields(value: Option<&Value>) -> Vec<FormField> {
    let Some(fields) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for field in fields {
        let Some(object) = field.as_object() else {
            continue;
        };
        let Some(key) = string_field(object, "key") else {
            continue;
        };
        let is_file = object.get("type").and_then(Value::as_str) == Some("file");
        out.push(FormField {
            key,
            value: if is_file {
                None
            } else {
                string_field(object, "value")
            },
            file: if is_file {
                string_field(object, "src")
            } else {
                None
            },
            content_type: string_field(object, "contentType"),
            description: string_field(object, "description"),
            enabled: !bool_field(object, "disabled").unwrap_or(false),
            extra: BTreeMap::new(),
        });
    }
    out
}

fn import_responses(value: Option<&Value>, path: &str, report: &mut ImportReport) -> Vec<Example> {
    let Some(responses) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (index, response) in responses.iter().enumerate() {
        let Some(object) = response.as_object() else {
            continue;
        };
        let code = match string_field(object, "code").and_then(|code| code.parse::<u16>().ok()) {
            Some(code) => code,
            None => {
                report.note(
                    format!("{path}[{index}].code"),
                    "saved response has no numeric code; recorded as 0",
                );
                0
            }
        };
        out.push(Example {
            name: string_field(object, "name").unwrap_or_else(|| "Example".to_owned()),
            // A Postman saved response names its request; the step's `Example` carries one, so
            // an empty Unsupported keeps the slot without inventing a request.
            request: Request::Unsupported(Value::Null),
            response: ExampleResponse {
                status: code,
                headers: import_headers(object.get("header")),
                body: string_field(object, "body").unwrap_or_default(),
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Auth

fn import_auth(value: &Value, path: &str, report: &mut ImportReport) -> Auth {
    let Some(object) = value.as_object() else {
        report.note(path, "auth is not an object; treated as inherit");
        return Auth::Inherit;
    };
    let kind = string_field(object, "type").unwrap_or_default();
    match kind.as_str() {
        "noauth" | "inherit" | "" => Auth::Inherit,
        "basic" => Auth::Basic {
            username: auth_field(object, "basic", "username"),
            password: auth_field(object, "basic", "password"),
            extra: auth_extra(object, "basic"),
        },
        "bearer" => Auth::Bearer {
            token: auth_field(object, "bearer", "token"),
            extra: auth_extra(object, "bearer"),
        },
        "apikey" => Auth::ApiKey {
            key: auth_field(object, "apikey", "key"),
            value: auth_field(object, "apikey", "value"),
            location: auth_field(object, "apikey", "in").and_then(|where_| match where_.as_str() {
                "header" => Some(ApiKeyLocation::Header),
                "query" => Some(ApiKeyLocation::Query),
                _ => None,
            }),
            extra: auth_extra(object, "apikey"),
        },
        other => {
            report.note(
                path,
                format!("auth type {other:?} preserved verbatim, not executed"),
            );
            Auth::Unsupported(Value::Object(object.clone()))
        }
    }
}

/// A Postman auth group is an ARRAY of `{key, value}` pairs; a plain object is also accepted
/// so a hand-written file maps too.
fn auth_field(object: &serde_json::Map<String, Value>, group: &str, name: &str) -> Option<String> {
    let group = object.get(group)?;
    if let Some(entries) = group.as_array() {
        return entries.iter().find_map(|entry| {
            let entry = entry.as_object()?;
            match string_field(entry, "key").as_deref() {
                Some(key) if key == name => string_field(entry, "value"),
                _ => None,
            }
        });
    }
    let fields = group.as_object()?;
    match fields.get(name)? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn auth_extra(object: &serde_json::Map<String, Value>, group: &str) -> BTreeMap<String, Value> {
    let mut extra = BTreeMap::new();
    let Some(group) = object.get(group) else {
        return extra;
    };
    let mut add = |key: &str, value: &Value| {
        if matches!(
            key,
            "username" | "password" | "token" | "key" | "value" | "in"
        ) {
            return;
        }
        extra.insert(key.to_owned(), value.clone());
    };
    match group {
        Value::Array(entries) => {
            for entry in entries {
                if let Some(entry) = entry.as_object()
                    && let Some(key) = string_field(entry, "key")
                    && let Some(value) = entry.get("value")
                {
                    add(&key, value);
                }
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                add(key, value);
            }
        }
        _ => {}
    }
    extra
}

// ---------------------------------------------------------------------------------------------
// Variables and events

fn split_secrets(
    root: &serde_json::Map<String, Value>,
    field: &str,
    report: &mut ImportReport,
) -> (Vec<Variable>, SecretValuesToStore) {
    let Some(values) = root.get(field).and_then(Value::as_array) else {
        return (Vec::new(), Vec::new());
    };
    let mut variables = Vec::new();
    let mut secrets = Vec::new();
    for (index, entry) in values.iter().enumerate() {
        let Some(object) = entry.as_object() else {
            report.note(
                format!("{field}[{index}]"),
                "value is not an object; skipped",
            );
            continue;
        };
        let Some(key) = string_field(object, "key") else {
            continue;
        };
        let value = string_field(object, "value");
        // Postman marks a secret with `type: "secret"`; older exports may omit the type on a
        // value whose key looks like a token, but only the declared type is trusted here — an
        // undeclared value is a normal shared value.
        if object.get("type").and_then(Value::as_str) == Some("secret") {
            match value {
                Some(value) if !value.is_empty() => secrets.push((key, value)),
                _ => report.note(
                    format!("{field}[{index}]"),
                    "a secret with no value is dropped: there is nothing to store",
                ),
            }
            continue;
        }
        variables.push(Variable {
            key,
            value,
            kind: VariableKind::Default,
            enabled: bool_field(object, "enabled").unwrap_or(true),
            description: string_field(object, "description"),
            extra: BTreeMap::new(),
        });
    }
    (variables, secrets)
}

fn import_variables(values: &[Value], path: &str, report: &mut ImportReport) -> Vec<Variable> {
    let mut out = Vec::new();
    for (index, entry) in values.iter().enumerate() {
        let Some(object) = entry.as_object() else {
            report.note(
                format!("{path}[{index}]"),
                "variable is not an object; skipped",
            );
            continue;
        };
        let Some(key) = string_field(object, "key") else {
            continue;
        };
        let is_secret = object.get("type").and_then(Value::as_str) == Some("secret");
        out.push(Variable {
            key,
            // A secret never keeps its value in the model (REQ-VAR-SHARE-02).
            value: if is_secret {
                None
            } else {
                string_field(object, "value")
            },
            kind: if is_secret {
                VariableKind::Secret
            } else {
                VariableKind::Default
            },
            enabled: bool_field(object, "enabled").unwrap_or(true),
            description: string_field(object, "description"),
            extra: BTreeMap::new(),
        });
    }
    out
}

/// Imported `event[]` scripts are preserved verbatim and **never executed** (plan D3/D4/D5).
fn import_events(
    object: &serde_json::Map<String, Value>,
    path: &str,
    extra: &mut BTreeMap<String, Value>,
    report: &mut ImportReport,
) {
    let Some(events) = object.get("event").and_then(Value::as_array) else {
        return;
    };
    for (index, event) in events.iter().enumerate() {
        let at = format!("{path}.event[{index}]");
        let listen = event
            .get("listen")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        report.note(
            &at,
            format!("{listen} script preserved verbatim, not executed"),
        );
    }
    extra.insert("event".to_owned(), Value::Array(events.clone()));
}

// ---------------------------------------------------------------------------------------------
// Export

fn export_children(children: &[crate::Child], out: &mut Vec<Value>, report: &mut ExportReport) {
    for child in children {
        match child {
            crate::Child::Folder(folder) => {
                let mut object = serde_json::Map::new();
                object.insert("name".to_owned(), Value::String(folder.name.clone()));
                if let Some(description) = &folder.description {
                    object.insert("description".to_owned(), Value::String(description.clone()));
                }
                if !Auth::is_inherit(&folder.auth) {
                    object.insert("auth".to_owned(), export_auth(&folder.auth));
                }
                let mut nested = Vec::new();
                export_children(&folder.children, &mut nested, report);
                object.insert("item".to_owned(), Value::Array(nested));
                for (key, value) in &folder.extra {
                    object.insert(key.clone(), value.clone());
                }
                out.push(Value::Object(object));
            }
            crate::Child::Request(Request::Http(http)) => {
                let mut object = serde_json::Map::new();
                object.insert("name".to_owned(), Value::String(http.name.clone()));
                object.insert("request".to_owned(), export_request(http));
                for (key, value) in &http.extra {
                    object.insert(key.clone(), value.clone());
                }
                out.push(Value::Object(object));
            }
            crate::Child::Request(Request::Unsupported(_)) => {
                report.warnings.push(
                    "a non-HTTP request kind was skipped: v2.1 has no shape for it".to_owned(),
                );
            }
        }
    }
}

fn export_request(http: &HttpRequest) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("method".to_owned(), Value::String(http.method.to_string()));
    object.insert("url".to_owned(), Value::String(http.url.clone()));
    if !http.headers.is_empty() {
        object.insert("header".to_owned(), export_key_values(&http.headers));
    }
    if !Body::is_none(&http.body) {
        object.insert("body".to_owned(), export_body(&http.body));
    }
    if !Auth::is_inherit(&http.auth) {
        object.insert("auth".to_owned(), export_auth(&http.auth));
    }
    Value::Object(object)
}

fn export_key_values(values: &[KeyValue]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|entry| {
                let mut object = serde_json::Map::new();
                object.insert("key".to_owned(), Value::String(entry.key.clone()));
                object.insert("value".to_owned(), optional(&entry.value));
                if !entry.enabled {
                    object.insert("disabled".to_owned(), Value::Bool(true));
                }
                Value::Object(object)
            })
            .collect(),
    )
}

fn export_form_fields(fields: &[FormField]) -> Value {
    Value::Array(
        fields
            .iter()
            .map(|field| {
                let mut object = serde_json::Map::new();
                object.insert("key".to_owned(), Value::String(field.key.clone()));
                if let Some(file) = &field.file {
                    object.insert("type".to_owned(), Value::String("file".to_owned()));
                    object.insert("src".to_owned(), Value::String(file.clone()));
                } else {
                    object.insert("value".to_owned(), optional(&field.value));
                }
                if !field.enabled {
                    object.insert("disabled".to_owned(), Value::Bool(true));
                }
                Value::Object(object)
            })
            .collect(),
    )
}

fn export_body(body: &Body) -> Value {
    let mut object = serde_json::Map::new();
    match body {
        Body::None => {}
        Body::Raw {
            language,
            text,
            extra,
        } => {
            object.insert("mode".to_owned(), Value::String("raw".to_owned()));
            object.insert("raw".to_owned(), Value::String(text.clone()));
            let mut raw = serde_json::Map::new();
            raw.insert(
                "language".to_owned(),
                Value::String(raw_language_name(language).to_owned()),
            );
            let mut options = serde_json::Map::new();
            options.insert("raw".to_owned(), Value::Object(raw));
            object.insert("options".to_owned(), Value::Object(options));
            for (key, value) in extra {
                object.insert(key.clone(), value.clone());
            }
        }
        Body::UrlEncoded { fields, extra } => {
            object.insert("mode".to_owned(), Value::String("urlencoded".to_owned()));
            object.insert("urlencoded".to_owned(), export_key_values(fields));
            for (key, value) in extra {
                object.insert(key.clone(), value.clone());
            }
        }
        Body::FormData { fields, extra } => {
            object.insert("mode".to_owned(), Value::String("formdata".to_owned()));
            object.insert("formdata".to_owned(), export_form_fields(fields));
            for (key, value) in extra {
                object.insert(key.clone(), value.clone());
            }
        }
        Body::Binary { file, extra } => {
            object.insert("mode".to_owned(), Value::String("file".to_owned()));
            let mut file_object = serde_json::Map::new();
            file_object.insert("src".to_owned(), Value::String(file.clone()));
            object.insert("file".to_owned(), Value::Object(file_object));
            for (key, value) in extra {
                object.insert(key.clone(), value.clone());
            }
        }
        Body::Unsupported(value) => return value.clone(),
    }
    Value::Object(object)
}

/// A Postman auth group is an array of `{key, value, type}` entries — the shape the v2.1 schema
/// requires (`definitions/auth-attribute`) and the shape real exports carry. A plain object is
/// accepted on import for hand-written files, but the export is always the canonical array.
fn auth_entry(key: &str, value: Value) -> Value {
    let mut entry = serde_json::Map::new();
    entry.insert("key".to_owned(), Value::String(key.to_owned()));
    entry.insert("value".to_owned(), value);
    entry.insert("type".to_owned(), Value::String("string".to_owned()));
    Value::Object(entry)
}

fn auth_group(named: &[(&str, &Option<String>)], extra: &BTreeMap<String, Value>) -> Value {
    let mut entries: Vec<Value> = named
        .iter()
        .map(|(key, value)| auth_entry(key, optional(value)))
        .collect();
    for (key, value) in extra {
        entries.push(auth_entry(key, value.clone()));
    }
    Value::Array(entries)
}

fn export_auth(auth: &Auth) -> Value {
    let mut object = serde_json::Map::new();
    match auth {
        Auth::Inherit => {
            object.insert("type".to_owned(), Value::String("inherit".to_owned()));
        }
        Auth::None => {
            object.insert("type".to_owned(), Value::String("noauth".to_owned()));
        }
        Auth::Basic {
            username,
            password,
            extra,
        } => {
            object.insert("type".to_owned(), Value::String("basic".to_owned()));
            object.insert(
                "basic".to_owned(),
                auth_group(&[("username", username), ("password", password)], extra),
            );
        }
        Auth::Bearer { token, extra } => {
            object.insert("type".to_owned(), Value::String("bearer".to_owned()));
            object.insert("bearer".to_owned(), auth_group(&[("token", token)], extra));
        }
        Auth::ApiKey {
            key,
            value,
            location,
            extra,
        } => {
            object.insert("type".to_owned(), Value::String("apikey".to_owned()));
            let where_ = location.map(|location| {
                match location {
                    ApiKeyLocation::Header => "header",
                    ApiKeyLocation::Query => "query",
                }
                .to_owned()
            });
            object.insert(
                "apikey".to_owned(),
                auth_group(&[("key", key), ("value", value), ("in", &where_)], extra),
            );
        }
        Auth::Unsupported(value) => return value.clone(),
    }
    Value::Object(object)
}

fn export_variable(variable: &Variable) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("key".to_owned(), Value::String(variable.key.clone()));
    // A secret never serializes its value (REQ-VAR-SHARE-02).
    if variable.kind != VariableKind::Secret {
        object.insert("value".to_owned(), optional(&variable.value));
    }
    object.insert(
        "type".to_owned(),
        Value::String(
            if variable.kind == VariableKind::Secret {
                "secret"
            } else {
                "string"
            }
            .to_owned(),
        ),
    );
    if !variable.enabled {
        object.insert("disabled".to_owned(), Value::Bool(true));
    }
    if let Some(description) = &variable.description {
        object.insert("description".to_owned(), Value::String(description.clone()));
    }
    Value::Object(object)
}

fn optional(value: &Option<String>) -> Value {
    match value {
        Some(text) => Value::String(text.clone()),
        None => Value::Null,
    }
}

// ---------------------------------------------------------------------------------------------
// Small field readers

fn as_object<'a>(
    value: &'a Value,
    what: &'static str,
) -> Result<&'a serde_json::Map<String, Value>, ImportError> {
    value.as_object().ok_or(ImportError::MissingField(what))
}

/// Read a string-named field the way the importer reads every key and name it maps: a
/// string as is, a number or a bool as its scalar text — Postman exports spell a key
/// either way. Public because the callers that walk the raw rows must agree with this
/// reader exactly (S45b fix 3).
pub fn string_field(object: &serde_json::Map<String, Value>, name: &str) -> Option<String> {
    match object.get(name)? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn bool_field(object: &serde_json::Map<String, Value>, name: &str) -> Option<bool> {
    match object.get(name)? {
        Value::Bool(flag) => Some(*flag),
        Value::String(text) => match text.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
#[path = "postman_tests.rs"]
mod tests;
