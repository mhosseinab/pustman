//! cURL import and export (PRD REQ-HTTP-13).
//!
//! `parse_curl` turns a pasted command into an `HttpRequest` and `to_curl` renders one back,
//! so a paste into the URL bar and "Copy as cURL" are the two directions of one conversion.
//!
//! Decisions (recorded in full in the step report):
//! - An unknown flag fails the parse with `CurlError::UnsupportedFlag` rather than being
//!   dropped, so nothing a user pasted is silently lost.
//! - `to_curl` writes POSIX single quotes: a value is wrapped in `'…'` and every `'` inside it
//!   becomes `'\''`, the one form a POSIX shell reads back byte for byte.
//! - `-d`/`--data`/`--data-binary` values starting with `@` name a file (curl reads the file),
//!   while `--data-raw` never does; `--compressed` is parked in `settings.extra` because
//!   Pustman asks for and decodes compressed responses itself; and a command without `-u` is
//!   one that sends no credentials, so it imports as `Auth::None`.

use std::collections::BTreeMap;
use std::fmt::{self, Display};

use serde_json::Value;

use crate::ItemId;
use crate::auth::{ApiKeyLocation, Auth};
use crate::request::{Body, FormField, HttpRequest, KeyValue, Method, RawLanguage};
use crate::settings::RequestSettings;

/// The `settings.extra` key `--compressed` is parked under: Pustman always accepts and decodes
/// compressed responses, so the flag has no field of its own.
const COMPRESSED: &str = "compressed";

/// What stopped a cURL command from becoming a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CurlError {
    /// The command carried no tokens at all.
    Empty,
    /// The quoting is unbalanced, so the command has no tokens.
    InvalidQuoting,
    /// A flag Pustman does not model. Reported rather than dropped.
    UnsupportedFlag(String),
    /// A flag that takes a value had none.
    MissingValue(String),
    /// The command named no URL.
    MissingUrl,
    /// A flag value is not shaped the way that flag needs.
    MalformedValue(String),
    /// `-X` carried something that is not a method token.
    InvalidMethod(String),
    /// A second bare argument after the URL.
    UnexpectedArgument(String),
}

impl Display for CurlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CurlError::Empty => f.write_str("the cURL command is empty"),
            CurlError::InvalidQuoting => f.write_str("the cURL command's quoting is unbalanced"),
            CurlError::UnsupportedFlag(flag) => write!(f, "unsupported cURL flag: {flag}"),
            CurlError::MissingValue(flag) => write!(f, "cURL flag {flag} needs a value"),
            CurlError::MissingUrl => f.write_str("the cURL command carries no URL"),
            CurlError::MalformedValue(value) => {
                write!(f, "not a value that cURL flag accepts: {value}")
            }
            CurlError::InvalidMethod(value) => write!(f, "not an HTTP method token: {value}"),
            CurlError::UnexpectedArgument(argument) => {
                write!(f, "unexpected argument after the URL: {argument}")
            }
        }
    }
}

impl std::error::Error for CurlError {}

/// Parse a pasted cURL command into a request.
///
/// Supported: `-X/--request`, `-H/--header`, `-d/--data/--data-raw/--data-binary/`
/// `--data-urlencode`, `-F/--form`, `-u/--user`, `-b/--cookie`, `-k/--insecure`, `-L/--location`
/// and `--compressed`, plus the `\` and `^` line continuations shells paste with. Any other
/// flag is reported in [`CurlError::UnsupportedFlag`].
pub fn parse_curl(command: &str) -> Result<HttpRequest, CurlError> {
    let tokens = shlex::split(&join_continuations(command)).ok_or(CurlError::InvalidQuoting)?;
    if tokens.is_empty() {
        return Err(CurlError::Empty);
    }

    let mut position = 0;
    if tokens
        .first()
        .is_some_and(|token| is_curl_invocation(token.as_str()))
    {
        position = 1;
    }

    let mut method: Option<Method> = None;
    let mut url: Option<String> = None;
    let mut headers: Vec<KeyValue> = Vec::new();
    let mut auth: Option<Auth> = None;
    let mut settings = RequestSettings::default();
    let mut settings_set = false;
    let mut data: Vec<DataPart> = Vec::new();
    let mut form: Vec<FormField> = Vec::new();

    while position < tokens.len() {
        let token = tokens[position].as_str();

        if let Some(body) = token.strip_prefix("--") {
            let (name, attached) = match body.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (body, None),
            };
            let flag = format!("--{name}");
            match name {
                "request" => {
                    let value = take_value(&tokens, &mut position, &flag, attached)?;
                    method = Some(parse_method(&value)?);
                }
                "header" => {
                    let value = take_value(&tokens, &mut position, &flag, attached)?;
                    headers.push(parse_header(&value)?);
                }
                "data" | "data-binary" => {
                    let value = take_value(&tokens, &mut position, &flag, attached)?;
                    data.push(parse_data(&value, true));
                }
                "data-raw" => {
                    let value = take_value(&tokens, &mut position, &flag, attached)?;
                    data.push(parse_data(&value, false));
                }
                "data-urlencode" => {
                    let value = take_value(&tokens, &mut position, &flag, attached)?;
                    data.push(parse_urlencoded(&value)?);
                }
                "form" => {
                    let value = take_value(&tokens, &mut position, &flag, attached)?;
                    form.push(parse_form(&value)?);
                }
                "user" => {
                    let value = take_value(&tokens, &mut position, &flag, attached)?;
                    auth = Some(parse_user(&value));
                }
                "cookie" => {
                    let value = take_value(&tokens, &mut position, &flag, attached)?;
                    headers.push(row("Cookie", Some(value)));
                }
                "insecure" => {
                    no_attached_value(&flag, attached)?;
                    settings.verify_tls = Some(false);
                    settings_set = true;
                }
                "location" => {
                    no_attached_value(&flag, attached)?;
                    settings.follow_redirects = Some(true);
                    settings_set = true;
                }
                "compressed" => {
                    no_attached_value(&flag, attached)?;
                    settings
                        .extra
                        .insert(COMPRESSED.to_string(), Value::Bool(true));
                    settings_set = true;
                }
                _ => return Err(CurlError::UnsupportedFlag(flag)),
            }
            position += 1;
            continue;
        }

        if token.len() > 1 && token.starts_with('-') {
            let characters: Vec<char> = token[1..].chars().collect();
            let mut cursor = 0;
            while cursor < characters.len() {
                let short = characters[cursor];
                let flag = format!("-{short}");
                let attached = {
                    let rest: String = characters[cursor + 1..].iter().collect();
                    (!rest.is_empty()).then_some(rest)
                };
                match short {
                    'X' | 'H' | 'd' | 'F' | 'u' | 'b' => {
                        let value = take_value(&tokens, &mut position, &flag, attached)?;
                        match short {
                            'X' => method = Some(parse_method(&value)?),
                            'H' => headers.push(parse_header(&value)?),
                            'd' => data.push(parse_data(&value, true)),
                            'F' => form.push(parse_form(&value)?),
                            'u' => auth = Some(parse_user(&value)),
                            _ => headers.push(row("Cookie", Some(value))),
                        }
                        cursor = characters.len();
                    }
                    'k' => {
                        settings.verify_tls = Some(false);
                        settings_set = true;
                        cursor += 1;
                    }
                    'L' => {
                        settings.follow_redirects = Some(true);
                        settings_set = true;
                        cursor += 1;
                    }
                    _ => return Err(CurlError::UnsupportedFlag(flag)),
                }
            }
            position += 1;
            continue;
        }

        if url.is_none() {
            url = Some(token.to_string());
        } else {
            return Err(CurlError::UnexpectedArgument(token.to_string()));
        }
        position += 1;
    }

    let url = url.ok_or(CurlError::MissingUrl)?;
    let params = query_params(&url);
    let method = match method {
        Some(method) => method,
        None if data.is_empty() && form.is_empty() => Method::Get,
        None => Method::Post,
    };
    let body = build_body(&data, &form, &headers)?;
    Ok(HttpRequest {
        id: ItemId::new(),
        name: url.clone(),
        method,
        url,
        params,
        path_variables: Vec::new(),
        headers,
        body,
        auth: auth.unwrap_or(Auth::None),
        settings: settings_set.then_some(settings),
        examples: Vec::new(),
        extra: BTreeMap::new(),
    })
}

/// Render a request as a cURL command [`parse_curl`] reads back.
///
/// Every value is POSIX single-quoted, disabled table rows stay out (they are not sent), and
/// auth kinds cURL has no flag for are written as the header they turn into on the wire.
pub fn to_curl(request: &HttpRequest) -> String {
    let mut query_pairs: Vec<(String, String)> = Vec::new();
    let mut auth_flags: Vec<String> = Vec::new();
    record_auth(&request.auth, &mut auth_flags, &mut query_pairs);

    let mut lines = vec![format!(
        "curl -X {} {}",
        request.method.as_str(),
        quote(&effective_url(request, &query_pairs))
    )];

    for header in request.headers.iter().filter(|header| header.enabled) {
        let value = header.value.as_deref().unwrap_or("");
        lines.push(format!(
            "-H {}",
            quote(&format!("{}: {}", header.key, value))
        ));
    }
    lines.extend(auth_flags);

    let settings = request.settings.as_ref();
    if settings.is_some_and(|settings| settings.verify_tls == Some(false)) {
        lines.push("-k".to_string());
    }
    if settings.is_some_and(|settings| settings.follow_redirects == Some(true)) {
        lines.push("-L".to_string());
    }
    if settings.is_some_and(|settings| settings.extra.get(COMPRESSED) == Some(&Value::Bool(true))) {
        lines.push("--compressed".to_string());
    }

    match &request.body {
        Body::None => {}
        Body::Raw { text, .. } => lines.push(format!("--data-raw {}", quote(text))),
        Body::UrlEncoded { fields, .. } => {
            for field in fields.iter().filter(|field| field.enabled) {
                let value = field.value.as_deref().unwrap_or("");
                lines.push(format!(
                    "--data-urlencode {}",
                    quote(&format!("{}={}", field.key, value))
                ));
            }
        }
        Body::Binary { file, .. } => {
            lines.push(format!("--data-binary {}", quote(&format!("@{file}"))));
        }
        Body::FormData { fields, .. } => {
            for field in fields.iter().filter(|field| field.enabled) {
                lines.push(format!("--form {}", quote(&form_spec(field))));
            }
        }
        Body::Unsupported(_) => {}
    }

    lines.join(" \\\n  ")
}

/// One `--data`/`--data-urlencode` argument before the body kind is decided.
enum DataPart {
    Text(String),
    File(String),
    UrlEncoded(String, String),
}

fn take_value(
    tokens: &[String],
    position: &mut usize,
    flag: &str,
    attached: Option<String>,
) -> Result<String, CurlError> {
    if let Some(value) = attached {
        return Ok(value);
    }
    *position += 1;
    tokens
        .get(*position)
        .cloned()
        .ok_or_else(|| CurlError::MissingValue(flag.to_string()))
}

fn no_attached_value(flag: &str, attached: Option<String>) -> Result<(), CurlError> {
    match attached {
        Some(value) => Err(CurlError::MalformedValue(format!(
            "{flag} takes no value: {value}"
        ))),
        None => Ok(()),
    }
}

fn parse_method(value: &str) -> Result<Method, CurlError> {
    Method::from_token(value).map_err(|_| CurlError::InvalidMethod(value.to_string()))
}

fn parse_header(text: &str) -> Result<KeyValue, CurlError> {
    let (key, value) = text
        .split_once(':')
        .ok_or_else(|| CurlError::MalformedValue(text.to_string()))?;
    Ok(row(key.trim(), Some(value.trim().to_string())))
}

fn parse_user(text: &str) -> Auth {
    // A lone username means curl would prompt for the password; there is none to store.
    let (username, password) = match text.split_once(':') {
        Some((username, password)) => (Some(username.to_string()), Some(password.to_string())),
        None => (Some(text.to_string()), None),
    };
    Auth::Basic {
        username,
        password,
        extra: BTreeMap::new(),
    }
}

fn parse_data(value: &str, allow_file: bool) -> DataPart {
    match (allow_file, value.strip_prefix('@')) {
        (true, Some(path)) => DataPart::File(path.to_string()),
        _ => DataPart::Text(value.to_string()),
    }
}

fn parse_urlencoded(value: &str) -> Result<DataPart, CurlError> {
    let (key, value) = value
        .split_once('=')
        .ok_or_else(|| CurlError::MalformedValue(value.to_string()))?;
    Ok(DataPart::UrlEncoded(key.to_string(), value.to_string()))
}

/// Parse one `-F` argument: `name=value` or `name=@file`, with `;type=<mime>` options.
fn parse_form(spec: &str) -> Result<FormField, CurlError> {
    let malformed = || CurlError::MalformedValue(spec.to_string());
    let (head, options) = match spec.split_once(';') {
        Some((head, options)) => (head, Some(options)),
        None => (spec, None),
    };
    let (key, raw) = head.split_once('=').ok_or_else(malformed)?;
    let (value, file) = match raw.strip_prefix('@') {
        Some(path) => (None, Some(path.to_string())),
        None => (Some(raw.to_string()), None),
    };

    let mut content_type = None;
    if let Some(options) = options {
        for option in options.split(';') {
            let (name, value) = option.split_once('=').ok_or_else(malformed)?;
            if name.trim() != "type" {
                return Err(malformed());
            }
            content_type = Some(value.trim().to_string());
        }
    }

    Ok(FormField {
        key: key.to_string(),
        value,
        file,
        content_type,
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    })
}

fn build_body(
    data: &[DataPart],
    form: &[FormField],
    headers: &[KeyValue],
) -> Result<Body, CurlError> {
    if !form.is_empty() {
        if !data.is_empty() {
            return Err(CurlError::MalformedValue(
                "-F cannot be combined with --data".to_string(),
            ));
        }
        return Ok(Body::FormData {
            fields: form.to_vec(),
            extra: BTreeMap::new(),
        });
    }
    if data.is_empty() {
        return Ok(Body::None);
    }
    if let [DataPart::File(path)] = data {
        return Ok(Body::Binary {
            file: path.clone(),
            extra: BTreeMap::new(),
        });
    }
    let urlencoded: Option<Vec<KeyValue>> = data
        .iter()
        .map(|part| match part {
            DataPart::UrlEncoded(key, value) => Some(row(key, Some(value.clone()))),
            DataPart::Text(_) | DataPart::File(_) => None,
        })
        .collect();
    if let Some(fields) = urlencoded {
        return Ok(Body::UrlEncoded {
            fields,
            extra: BTreeMap::new(),
        });
    }
    if data.iter().any(|part| matches!(part, DataPart::File(_))) {
        return Err(CurlError::MalformedValue(
            "@file cannot be combined with other data arguments".to_string(),
        ));
    }

    let text = data
        .iter()
        .map(data_part_text)
        .collect::<Vec<_>>()
        .join("&");
    Ok(Body::Raw {
        language: content_type_language(headers),
        text,
        extra: BTreeMap::new(),
    })
}

fn data_part_text(part: &DataPart) -> String {
    match part {
        DataPart::Text(value) => value.clone(),
        DataPart::File(path) => format!("@{path}"),
        DataPart::UrlEncoded(key, value) => format!("{key}={value}"),
    }
}

/// The language a raw body is edited in follows its Content-Type (REQ-HTTP-04).
fn content_type_language(headers: &[KeyValue]) -> RawLanguage {
    let content_type = headers.iter().find_map(|header| {
        if header.enabled && header.key.eq_ignore_ascii_case("content-type") {
            header.value.as_deref()
        } else {
            None
        }
    });
    let Some(content_type) = content_type else {
        return RawLanguage::Text;
    };
    let content_type = content_type.to_ascii_lowercase();
    if content_type.contains("json") {
        RawLanguage::Json
    } else if content_type.contains("xml") {
        RawLanguage::Xml
    } else if content_type.contains("html") {
        RawLanguage::Html
    } else if content_type.contains("javascript") || content_type.contains("ecmascript") {
        RawLanguage::JavaScript
    } else {
        RawLanguage::Text
    }
}

/// The auth as the flags cURL carries it in: `-u` for Basic, and the header or query pair any
/// other kind turns into on the wire.
fn record_auth(auth: &Auth, flags: &mut Vec<String>, query_pairs: &mut Vec<(String, String)>) {
    match auth {
        Auth::Basic {
            username, password, ..
        } => {
            let username = username.as_deref().unwrap_or("");
            let credentials = match password {
                Some(password) => format!("{username}:{password}"),
                None => username.to_string(),
            };
            flags.push(format!("-u {}", quote(&credentials)));
        }
        Auth::Bearer { token, .. } => {
            if let Some(token) = token {
                flags.push(format!(
                    "-H {}",
                    quote(&format!("Authorization: Bearer {token}"))
                ));
            }
        }
        Auth::ApiKey {
            key,
            value,
            location,
            ..
        } => {
            if let (Some(key), Some(value)) = (key, value) {
                match location.unwrap_or(ApiKeyLocation::Header) {
                    ApiKeyLocation::Header => {
                        flags.push(format!("-H {}", quote(&format!("{key}: {value}"))));
                    }
                    ApiKeyLocation::Query => {
                        query_pairs.push((key.clone(), value.clone()));
                    }
                }
            }
        }
        Auth::Inherit | Auth::None | Auth::Unsupported(_) => {}
    }
}

/// The `-F` argument one form field turns back into.
fn form_spec(field: &FormField) -> String {
    let mut spec = match (&field.value, &field.file) {
        (Some(value), _) => format!("{}={}", field.key, value),
        (None, Some(file)) => format!("{}=@{}", field.key, file),
        (None, None) => format!("{}=", field.key),
    };
    if let Some(content_type) = &field.content_type {
        spec.push_str(";type=");
        spec.push_str(content_type);
    }
    spec
}

/// The URL a request sends: the stored URL with the Params table merged in (REQ-HTTP-02 keeps
/// the two in sync, so this only adds rows the query does not carry yet).
fn effective_url(request: &HttpRequest, extra_pairs: &[(String, String)]) -> String {
    let (base, fragment) = match request.url.split_once('#') {
        Some((base, fragment)) => (base, Some(fragment)),
        None => (request.url.as_str(), None),
    };
    let (path, query) = match base.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (base, None),
    };
    let raw: Vec<&str> = query
        .map(|query| query.split('&').filter(|pair| !pair.is_empty()).collect())
        .unwrap_or_default();
    let present: Vec<&str> = raw.iter().map(|pair| pair_key(pair)).collect();
    let mut pairs: Vec<String> = raw.into_iter().map(str::to_string).collect();

    for param in request.params.iter().filter(|param| param.enabled) {
        if present.contains(&param.key.as_str()) {
            continue;
        }
        pairs.push(format!(
            "{}={}",
            param.key,
            param.value.as_deref().unwrap_or("")
        ));
    }
    for (key, value) in extra_pairs {
        if present.contains(&key.as_str()) {
            continue;
        }
        pairs.push(format!("{key}={value}"));
    }

    let mut url = path.to_string();
    if !pairs.is_empty() {
        url.push('?');
        url.push_str(&pairs.join("&"));
    }
    if let Some(fragment) = fragment {
        url.push('#');
        url.push_str(fragment);
    }
    url
}

/// The `key=value` pairs a URL's query carries, in order.
fn query_pairs(url: &str) -> Vec<&str> {
    let query = match url.split_once('?') {
        Some((_, query)) => query,
        None => return Vec::new(),
    };
    let query = match query.split_once('#') {
        Some((query, _)) => query,
        None => query,
    };
    query.split('&').filter(|pair| !pair.is_empty()).collect()
}

fn pair_key(pair: &str) -> &str {
    match pair.split_once('=') {
        Some((key, _)) => key,
        None => pair,
    }
}

/// The Params table rows a URL's query mirrors into (REQ-HTTP-02).
fn query_params(url: &str) -> Vec<KeyValue> {
    query_pairs(url)
        .into_iter()
        .map(|pair| {
            let (key, value) = match pair.split_once('=') {
                Some((key, value)) => (key, Some(value.to_string())),
                None => (pair, None),
            };
            row(key, value)
        })
        .collect()
}

fn row(key: &str, value: Option<String>) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value,
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
}

/// POSIX single-quote a value: wrap it in `'…'` and write every `'` inside as `'\''`
/// (close, escaped quote, reopen), which a POSIX shell reads back byte for byte.
fn quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

/// Join the `\` and `^` line continuations shells paste with. `shlex` joins an unquoted
/// `\<newline>` on its own, but a continuation inside a quoted value has to survive untouched
/// and `^` (the cmd.exe form) is not a POSIX escape at all, so the join happens here where the
/// quoting is known.
fn join_continuations(command: &str) -> String {
    let mut joined = String::new();
    let mut open_quote: Option<char> = None;
    for line in command.lines() {
        open_quote = quote_state(line, open_quote);
        if open_quote.is_none()
            && let Some(continued) = continuation_target(line)
        {
            joined.push_str(continued);
            continue;
        }
        joined.push_str(line);
        joined.push('\n');
    }
    joined
}

/// The line with its continuation marker removed, when it ends with one. A trailing `\\` is an
/// escaped backslash and the newline stays a separator, so only an odd run continues.
fn continuation_target(line: &str) -> Option<&str> {
    let backslashes = line
        .chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count();
    if backslashes % 2 == 1 {
        return line.strip_suffix('\\');
    }
    line.strip_suffix('^')
}

/// The quote `text` leaves `open` in: `'` and `"` toggle, and outside a single quote a
/// backslash escapes whatever follows it.
fn quote_state(text: &str, mut open: Option<char>) -> Option<char> {
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        match open {
            Some('\'') => {
                if character == '\'' {
                    open = None;
                }
            }
            Some(_) => {
                if character == '"' {
                    open = None;
                } else if character == '\\' {
                    characters.next();
                }
            }
            None => {
                if character == '\'' {
                    open = Some('\'');
                } else if character == '"' {
                    open = Some('"');
                } else if character == '\\' {
                    characters.next();
                }
            }
        }
    }
    open
}

/// A paste usually starts with the program itself; when it names curl it is dropped so the
/// first real argument is the URL.
fn is_curl_invocation(token: &str) -> bool {
    let name = match token.rsplit_once(['/', '\\']) {
        Some((_, name)) => name,
        None => token,
    };
    let name = name.strip_suffix(".exe").unwrap_or(name);
    name.eq_ignore_ascii_case("curl")
}

#[cfg(test)]
#[path = "curl_tests.rs"]
mod tests;
