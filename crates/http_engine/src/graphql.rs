//! GraphQL over the HTTP engine (REQ-GQL-01): a [`GraphQlRequest`] goes on the wire as
//! `POST application/json {query, variables, operationName}` through the normal pipeline.
//!
//! The body is built as raw JSON text with the variables editor's text spliced in as
//! typed, so `{{variables}}` in the query and the variables resolve in [`crate::prepare`]
//! exactly as they do in an HTTP request's raw body — including an unquoted
//! `{"id": {{userId}}}`, which only becomes JSON once resolved.

use std::collections::BTreeMap;
use std::sync::Arc;

use api_model::{Auth, Body, GraphQlRequest, HttpRequest, Method, RawLanguage};
use secrets::SecretStore;
use variables::{ScopeStack, spans};

use crate::SecretKeyLookup;

#[cfg(test)]
#[path = "graphql_tests.rs"]
mod tests;

/// Everything one [`crate::HttpClient::send_graphql`] needs: [`crate::SendInput`] with the
/// GraphQL request in place of the HTTP one it is sent as.
pub struct GraphQlSendInput {
    /// The request to send, unresolved: `{{variables}}` in the URL, headers, query and
    /// variables resolve in [`crate::prepare`].
    pub request: GraphQlRequest,
    /// The request's parents' auths, nearest first (folder → … → collection).
    pub ancestors: Vec<Auth>,
    /// The variables the request resolves against.
    pub scopes: ScopeStack,
    /// Where a referenced secret's value comes from.
    pub secrets: Arc<dyn SecretStore>,
    /// The keychain key of a secret variable, as in [`crate::SendInput::secret_key`].
    pub secret_key: SecretKeyLookup,
}

/// The protocol a GraphQL send is listed under in History and the Console (REQ-GQL-01).
pub const PROTOCOL: &str = "graphql";

/// Why a [`GraphQlRequest`] cannot be built into an HTTP request.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GraphQlError {
    /// The variables editor's text is not a JSON value. `line` and `column` are 1-based
    /// and point into the text as the user typed it, so the editor can mark the spot.
    #[error(
        "The GraphQL variables are not valid JSON (line {line}, column {column}): {reason}. \
         Fix the variables, or clear them to send none."
    )]
    InvalidVariables {
        line: usize,
        column: usize,
        reason: String,
    },
}

/// The HTTP request `request` is sent as: `POST` with a raw JSON body
/// `{query, variables, operationName}`, the request's URL, headers, auth and settings.
/// Empty variables and an empty operation name are left out of the body, which the
/// GraphQL-over-HTTP spec allows for both.
///
/// # Errors
///
/// [`GraphQlError::InvalidVariables`] when the variables text is not JSON once every
/// `{{placeholder}}` is read as a value.
pub fn to_http_request(request: &GraphQlRequest) -> Result<HttpRequest, GraphQlError> {
    let variables = request.variables.trim();
    if !variables.is_empty() {
        validate_variables(&request.variables)?;
    }
    let mut body = String::from("{\"query\":");
    body.push_str(&json_string(&request.query));
    if !variables.is_empty() {
        body.push_str(",\"variables\":");
        body.push_str(variables);
    }
    if let Some(operation_name) = request
        .operation_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    {
        body.push_str(",\"operationName\":");
        body.push_str(&json_string(operation_name));
    }
    body.push('}');
    Ok(HttpRequest {
        id: request.id,
        name: request.name.clone(),
        method: Method::Post,
        url: request.url.clone(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: request.headers.clone(),
        body: Body::Raw {
            language: RawLanguage::Json,
            text: body,
            extra: BTreeMap::new(),
        },
        auth: request.auth.clone(),
        settings: request.settings.clone(),
        examples: Vec::new(),
        extra: BTreeMap::new(),
    })
}

/// `text` as a JSON string literal.
fn json_string(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

/// Check `variables` is JSON with every `{{placeholder}}` standing for a value. Each
/// placeholder is overwritten in place by `0` and spaces — the same byte length, so the
/// reported line and column still point into the text as typed — which is a valid value
/// outside a string and valid text inside one.
fn validate_variables(variables: &str) -> Result<(), GraphQlError> {
    let mut masked = variables.to_string();
    for span in spans(variables, &ScopeStack::new()).into_iter().rev() {
        let filler = format!("0{}", " ".repeat(span.range.len().saturating_sub(1)));
        masked.replace_range(span.range, &filler);
    }
    match serde_json::from_slice::<serde_json::Value>(masked.as_bytes()) {
        Ok(_) => Ok(()),
        Err(error) => Err(GraphQlError::InvalidVariables {
            line: error.line(),
            column: error.column(),
            reason: reason_of(&error),
        }),
    }
}

/// serde_json's message without its own " at line L column C" suffix, which the
/// [`GraphQlError`] message states once already.
fn reason_of(error: &serde_json::Error) -> String {
    let message = error.to_string();
    let suffix = format!(" at line {} column {}", error.line(), error.column());
    message
        .strip_suffix(&suffix)
        .unwrap_or(&message)
        .to_string()
}

/// The names of the named operations in `document`, in document order — what the
/// operation-name picker lists (REQ-GQL-01). Anonymous operations, fragments and
/// anything inside a selection set, an argument list, a string or a comment are not
/// operations and are skipped.
pub fn operation_names(document: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut tokens = Tokens::new(document);
    let mut depth = 0usize;
    let mut state = Definition::Expecting;
    while let Some(token) = tokens.next() {
        match token {
            Token::Open => {
                if depth == 0 {
                    state = Definition::Inside;
                }
                depth += 1;
            }
            Token::Close => {
                depth = depth.saturating_sub(1);
                // A top-level selection set closing ends the definition; a closing
                // argument list (`query Q($a: Int)`) does not, but the selection set
                // that follows it does.
                if depth == 0 && tokens.closed_brace {
                    state = Definition::Expecting;
                }
            }
            Token::Name(name) if depth == 0 => {
                state = match state {
                    Definition::Expecting
                        if matches!(name, "query" | "mutation" | "subscription") =>
                    {
                        Definition::AwaitingName
                    }
                    Definition::AwaitingName => {
                        names.push(name.to_string());
                        Definition::Inside
                    }
                    _ => Definition::Inside,
                };
            }
            Token::Name(_) | Token::Other => {
                if depth == 0 && state == Definition::AwaitingName {
                    state = Definition::Inside;
                }
            }
        }
    }
    names
}

/// Where the scanner is between top-level definitions.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Definition {
    /// The next top-level name starts a definition.
    Expecting,
    /// An operation keyword was read; a name here is the operation's name.
    AwaitingName,
    /// Inside a definition that has not closed its selection set yet.
    Inside,
}

/// One lexical token of a GraphQL document, as far as [`operation_names`] cares.
enum Token<'a> {
    Name(&'a str),
    /// `{` or `(`.
    Open,
    /// `}` or `)`.
    Close,
    /// Any other punctuation, number or string.
    Other,
}

/// A GraphQL lexer that skips whitespace, commas, `#` comments and string values
/// (`"…"` with escapes, and `"""…"""` block strings).
struct Tokens<'a> {
    text: &'a str,
    position: usize,
    /// Whether the last [`Token::Close`] was a `}` rather than a `)`.
    closed_brace: bool,
}

impl<'a> Tokens<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            position: 0,
            closed_brace: false,
        }
    }

    fn rest(&self) -> &'a str {
        self.text.get(self.position..).unwrap_or("")
    }

    /// Advance past a string value that starts at the current position.
    fn skip_string(&mut self) {
        if self.rest().starts_with("\"\"\"") {
            let body = self.position + 3;
            let mut cursor = body;
            loop {
                let Some(found) = self.text.get(cursor..).and_then(|rest| rest.find("\"\"\""))
                else {
                    self.position = self.text.len();
                    return;
                };
                let at = cursor + found;
                // `\"""` is an escaped triple quote inside a block string.
                if self
                    .text
                    .get(..at)
                    .is_some_and(|before| before.ends_with('\\'))
                {
                    cursor = at + 3;
                    continue;
                }
                self.position = at + 3;
                return;
            }
        }
        let mut escaped = false;
        for (offset, character) in self.rest().char_indices().skip(1) {
            match character {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                // A line break ends an unterminated string, as the GraphQL lexer does.
                '"' | '\n' | '\r' => {
                    self.position += offset + character.len_utf8();
                    return;
                }
                _ => {}
            }
        }
        self.position = self.text.len();
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        loop {
            let character = self.rest().chars().next()?;
            match character {
                '#' => {
                    let line_end = self.rest().find(['\n', '\r']).unwrap_or(self.rest().len());
                    self.position += line_end;
                }
                '"' => {
                    self.skip_string();
                    return Some(Token::Other);
                }
                '{' | '(' => {
                    self.position += 1;
                    return Some(Token::Open);
                }
                '}' | ')' => {
                    self.position += 1;
                    self.closed_brace = character == '}';
                    return Some(Token::Close);
                }
                _ if character == '_' || character.is_ascii_alphabetic() => {
                    let rest = self.rest();
                    let length = rest
                        .find(|c: char| c != '_' && !c.is_ascii_alphanumeric())
                        .unwrap_or(rest.len());
                    self.position += length;
                    return rest.get(..length).map(Token::Name);
                }
                _ if character.is_whitespace() || character == ',' || character == '\u{feff}' => {
                    self.position += character.len_utf8();
                }
                _ => {
                    self.position += character.len_utf8();
                    return Some(Token::Other);
                }
            }
        }
    }
}
