//! What the body's right-click menu copies from a Pretty JSON line: the property path
//! of the clicked token, its value, and — for strings — the string in a few forms.
//!
//! It works from the formatted lines and their cached syntax spans, so nothing is parsed
//! again on a click, and a copied container is the text the pane shows.

use gpui::SharedString;

use crate::body_selection::BodyPoint;
use crate::json_syntax::{JsonToken, LineSpans};

/// One step of a property path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PathStep {
    Key(String),
    Index(usize),
}

/// What a right-click on a JSON line points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JsonTarget {
    /// Where the value sits in the document; empty for the root.
    pub path: Vec<PathStep>,
    /// The value: a string's contents, a primitive's literal, a container's text.
    pub value: String,
    /// The clicked string's contents, when a key or string value was clicked.
    pub string: Option<String>,
}

/// One open container on the way down to the clicked token.
enum Frame {
    Object { key: Option<String> },
    Array { index: usize },
}

/// A token's position: its line and its index in that line's spans.
#[derive(Clone, Copy, PartialEq, Eq)]
struct TokenAt {
    line: usize,
    span: usize,
}

/// The JSON target under `at` in the formatted `lines`. A point between tokens targets
/// the next token on its line (or its last one), so a click in the indentation still
/// names the member the line holds.
pub(crate) fn json_target_at(
    lines: &[SharedString],
    spans: &[LineSpans],
    at: BodyPoint,
) -> Option<JsonTarget> {
    let line_spans = spans.get(at.line)?;
    let under = line_spans
        .iter()
        .position(|(range, _)| at.column < range.end)
        .or_else(|| line_spans.len().checked_sub(1))?;
    // A separator names nothing; the line's first token is the member it ends.
    let clicked_span = match lines.get(at.line)?.get(line_spans.get(under)?.0.clone()) {
        Some("," | ":") => 0,
        _ => under,
    };
    let clicked = TokenAt {
        line: at.line,
        span: clicked_span,
    };
    let mut frames: Vec<Frame> = Vec::new();
    let mut tokens = TokenWalk::new(spans);
    while let Some(position) = tokens.next() {
        let (_, token) = spans.get(position.line)?.get(position.span)?;
        let text = token_text(lines, spans, position)?;
        if position == clicked {
            return target_for(
                lines,
                spans,
                &mut frames,
                &mut tokens,
                position,
                *token,
                text,
            );
        }
        step(&mut frames, *token, text);
    }
    None
}

/// The target once the walk reached the clicked token.
fn target_for(
    lines: &[SharedString],
    spans: &[LineSpans],
    frames: &mut Vec<Frame>,
    tokens: &mut TokenWalk,
    clicked: TokenAt,
    token: JsonToken,
    text: &str,
) -> Option<JsonTarget> {
    match token {
        JsonToken::Key => {
            let key = unescape(text)?;
            if let Some(Frame::Object { key: slot }) = frames.last_mut() {
                *slot = Some(key.clone());
            }
            // A key is followed by its colon, then by its value.
            tokens.next()?;
            let value_at = tokens.next()?;
            Some(JsonTarget {
                path: path_of(frames),
                value: value_text(lines, spans, value_at)?,
                string: Some(key),
            })
        }
        JsonToken::Punctuation if text == "}" || text == "]" => {
            let open = matching_open(spans, lines, clicked)?;
            frames.pop();
            Some(JsonTarget {
                path: path_of(frames),
                value: value_text(lines, spans, open)?,
                string: None,
            })
        }
        _ => Some(JsonTarget {
            path: path_of(frames),
            value: value_text(lines, spans, clicked)?,
            string: (token == JsonToken::String)
                .then(|| unescape(text))
                .flatten(),
        }),
    }
}

/// Advance the container frames past one token.
fn step(frames: &mut Vec<Frame>, token: JsonToken, text: &str) {
    match (token, text) {
        (JsonToken::Punctuation, "{") => frames.push(Frame::Object { key: None }),
        (JsonToken::Punctuation, "[") => frames.push(Frame::Array { index: 0 }),
        (JsonToken::Punctuation, "}" | "]") => {
            frames.pop();
        }
        (JsonToken::Punctuation, ",") => match frames.last_mut() {
            Some(Frame::Array { index }) => *index += 1,
            Some(Frame::Object { key }) => *key = None,
            None => {}
        },
        (JsonToken::Key, text) => {
            if let Some(Frame::Object { key }) = frames.last_mut() {
                *key = unescape(text);
            }
        }
        _ => {}
    }
}

/// The path the open frames spell out.
fn path_of(frames: &[Frame]) -> Vec<PathStep> {
    frames
        .iter()
        .map_while(|frame| match frame {
            Frame::Object { key } => key.clone().map(PathStep::Key),
            Frame::Array { index } => Some(PathStep::Index(*index)),
        })
        .collect()
}

/// The value starting at `start`: a string's contents, a primitive's literal, or a
/// container's text from its opening bracket to its closing one, dedented by the
/// opening line's indentation.
fn value_text(lines: &[SharedString], spans: &[LineSpans], start: TokenAt) -> Option<String> {
    let (range, token) = spans.get(start.line)?.get(start.span)?;
    let text = token_text(lines, spans, start)?;
    match token {
        JsonToken::String | JsonToken::Key => unescape(text),
        JsonToken::Punctuation if text == "{" || text == "[" => {
            let close = matching_close(spans, lines, start)?;
            let first = lines.get(start.line)?;
            let indent = first.len() - first.trim_start().len();
            let mut copied = String::new();
            for index in start.line..=close.line {
                let line = lines.get(index)?;
                let from = if index == start.line {
                    range.start
                } else {
                    indent.min(line.len() - line.trim_start().len())
                };
                let to = if index == close.line {
                    spans.get(close.line)?.get(close.span)?.0.end
                } else {
                    line.len()
                };
                if index > start.line {
                    copied.push('\n');
                }
                copied.push_str(line.get(from..to)?);
            }
            Some(copied)
        }
        _ => Some(text.to_string()),
    }
}

/// The bracket closing the container opened at `open`.
fn matching_close(spans: &[LineSpans], lines: &[SharedString], open: TokenAt) -> Option<TokenAt> {
    let mut depth = 0usize;
    TokenWalk::starting_at(spans, open).find(|&position| {
        match token_text(lines, spans, position) {
            Some("{" | "[") => depth += 1,
            Some("}" | "]") => depth = depth.saturating_sub(1),
            _ => return false,
        }
        depth == 0
    })
}

/// The bracket opening the container closed at `close`.
fn matching_open(spans: &[LineSpans], lines: &[SharedString], close: TokenAt) -> Option<TokenAt> {
    let mut open: Vec<TokenAt> = Vec::new();
    for position in TokenWalk::new(spans) {
        match token_text(lines, spans, position) {
            Some("{" | "[") => open.push(position),
            Some("}" | "]") => {
                let opened = open.pop();
                if position == close {
                    return opened;
                }
            }
            _ => {}
        }
    }
    None
}

/// The source text of one token.
fn token_text<'a>(lines: &'a [SharedString], spans: &[LineSpans], at: TokenAt) -> Option<&'a str> {
    let (range, _) = spans.get(at.line)?.get(at.span)?;
    lines.get(at.line)?.get(range.clone())
}

/// A JSON string token's contents.
fn unescape(token: &str) -> Option<String> {
    serde_json::from_str::<String>(token).ok()
}

/// Every token of the lines in order, as positions.
struct TokenWalk<'a> {
    spans: &'a [LineSpans],
    next: TokenAt,
}

impl<'a> TokenWalk<'a> {
    fn new(spans: &'a [LineSpans]) -> Self {
        Self::starting_at(spans, TokenAt { line: 0, span: 0 })
    }

    fn starting_at(spans: &'a [LineSpans], next: TokenAt) -> Self {
        Self { spans, next }
    }
}

impl Iterator for TokenWalk<'_> {
    type Item = TokenAt;

    fn next(&mut self) -> Option<TokenAt> {
        loop {
            let line = self.spans.get(self.next.line)?;
            if self.next.span < line.len() {
                let position = self.next;
                self.next.span += 1;
                return Some(position);
            }
            self.next = TokenAt {
                line: self.next.line + 1,
                span: 0,
            };
        }
    }
}

/// A DevTools-style property path: `data[0].name`, with keys that are not identifiers
/// bracketed as `["a-b"]`.
pub(crate) fn property_path(path: &[PathStep]) -> String {
    let mut text = String::new();
    for step in path {
        match step {
            PathStep::Index(index) => text.push_str(&format!("[{index}]")),
            PathStep::Key(key) if is_identifier(key) => {
                if !text.is_empty() {
                    text.push('.');
                }
                text.push_str(key);
            }
            PathStep::Key(key) => {
                text.push('[');
                text.push_str(&json_string_literal(key));
                text.push(']');
            }
        }
    }
    text
}

fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
        && chars.all(|rest| rest.is_alphanumeric() || rest == '_' || rest == '$')
}

/// `text` as a JSON string literal.
pub(crate) fn json_string_literal(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

/// `text` as a single-quoted JavaScript string literal.
pub(crate) fn js_string_literal(text: &str) -> String {
    let mut literal = String::with_capacity(text.len() + 2);
    literal.push('\'');
    for character in text.chars() {
        match character {
            '\\' => literal.push_str("\\\\"),
            '\'' => literal.push_str("\\'"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\t' => literal.push_str("\\t"),
            '\u{2028}' | '\u{2029}' => literal.push_str(&format!("\\u{:04x}", character as u32)),
            control if control.is_control() => {
                literal.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => literal.push(other),
        }
    }
    literal.push('\'');
    literal
}

#[cfg(test)]
#[path = "json_copy_tests.rs"]
mod tests;
