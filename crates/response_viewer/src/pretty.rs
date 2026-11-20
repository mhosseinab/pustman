//! The pure formatters behind Pretty mode (REQ-RESP-02): JSON pretty-printed, XML and
//! HTML indented by tag depth, everything else left as the text it is.
//!
//! Nothing here knows about GPUI: formatting a large body is heavy work, so the body
//! view runs it off the foreground and this module only sees bytes.

use std::fmt;

/// The formatter for a body. The `Content-Type` header picks it ([`Formatter::detect`])
/// unless the user overrides it by hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Formatter {
    /// JSON, pretty-printed with two-space indentation; key order survives (`serde_json`
    /// runs with `preserve_order`).
    Json,
    /// XML, indented by tag depth.
    Xml,
    /// HTML, indented by tag depth; void elements never open a level.
    Html,
    /// Anything else: the text exactly, the way Raw shows it.
    Plain,
}

/// Why a body could not be formatted. The view falls back to the Raw text and says so —
/// a body it cannot parse is data, never a panic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatError {
    /// The bytes are not valid UTF-8, so there is no text to format.
    InvalidUtf8,
    /// The text is not valid for the formatter — malformed JSON, for example.
    Malformed,
    /// The formatted body would grow past the output budget: deeply nested markup can
    /// indent quadratically, so it is refused instead of allocated.
    TooLarge,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 => f.write_str("the body is not valid UTF-8"),
            Self::Malformed => f.write_str("the body is not valid for this formatter"),
            Self::TooLarge => f.write_str("the formatted body would be too large"),
        }
    }
}

impl Formatter {
    /// The formatter a `Content-Type` header value asks for. A `+json` or `+xml` suffix
    /// counts too (RFC 6839); a missing or unparseable header means [`Formatter::Plain`].
    pub fn detect(content_type: Option<&str>) -> Self {
        let Some(value) = content_type else {
            return Self::Plain;
        };
        let Ok(parsed) = value.parse::<mime::Mime>() else {
            return Self::Plain;
        };
        if parsed.subtype() == mime::JSON || parsed.suffix() == Some(mime::JSON) {
            Self::Json
        } else if parsed.subtype() == mime::XML || parsed.suffix() == Some(mime::XML) {
            Self::Xml
        } else if parsed.subtype() == mime::HTML {
            Self::Html
        } else {
            Self::Plain
        }
    }

    /// The name the formatter override menu and the fallback note show.
    pub fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Xml => "XML",
            Self::Html => "HTML",
            Self::Plain => "Plain",
        }
    }

    /// Format `bytes` the way this formatter shows them. JSON must parse and every
    /// formatter needs UTF-8 text; anything else is a [`FormatError`] the view falls
    /// back from.
    pub fn format(self, bytes: &[u8]) -> Result<String, FormatError> {
        let text = std::str::from_utf8(bytes).map_err(|_| FormatError::InvalidUtf8)?;
        match self {
            Self::Json => {
                let value: serde_json::Value =
                    serde_json::from_slice(bytes).map_err(|_| FormatError::Malformed)?;
                serde_json::to_string_pretty(&value).map_err(|_| FormatError::Malformed)
            }
            Self::Xml => indent_markup(text, &[]),
            Self::Html => indent_markup(text, HTML_VOID_ELEMENTS),
            Self::Plain => Ok(text.to_owned()),
        }
    }
}

/// The HTML elements that close themselves: they never open a level (the WHATWG void
/// elements, trimmed to the ones real bodies carry).
const HTML_VOID_ELEMENTS: &[&str] = &["br", "img", "meta", "link", "input", "hr"];

/// The deepest level that gets its own indent: a hostile body of unclosed tags must not
/// make the indent prefixes grow without end (they would expand the output quadratically
/// in the depth). Levels past this share the cap's indent.
const MAX_INDENT_DEPTH: usize = 32;

/// How much bigger than the input the formatted markup may get. The budget makes the
/// output linear in the input even where the indent cap is not enough; past it the
/// formatter returns [`FormatError::TooLarge`] and the view shows the Raw text.
const OUTPUT_BUDGET_FACTOR: usize = 4;

/// Re-indent markup by tag depth: one tag or text run per line, two spaces per level.
/// This is the spec's "simple XML/HTML indent" — no parser and no validation. `<?…?>`,
/// `<!…>` and self-closing tags stay on their own line without opening a level, and void
/// HTML elements never open one either. Whitespace inside text runs and tags collapses
/// to single spaces (Raw keeps the bytes exact), and a tag left unfinished at the end of
/// the body is shown as the text it is.
///
/// The result stays linear in the input: the indent stops deepening at
/// [`MAX_INDENT_DEPTH`] and the output is refused past
/// `text.len() * `[`OUTPUT_BUDGET_FACTOR`] — response bodies are untrusted data.
fn indent_markup(text: &str, void_elements: &[&str]) -> Result<String, FormatError> {
    let mut lines: Vec<String> = Vec::new();
    let mut budget = text.len().saturating_mul(OUTPUT_BUDGET_FACTOR);
    let mut depth: usize = 0;
    let mut rest = text;
    while !rest.is_empty() {
        let Some(start) = rest.find('<') else {
            push_line(&mut lines, rest, depth, &mut budget)?;
            break;
        };
        if start > 0 {
            push_line(&mut lines, &rest[..start], depth, &mut budget)?;
        }
        let Some((tag, remainder)) = take_tag(&rest[start..]) else {
            // No '>' ever closes this tag: show the rest as the text it is.
            push_line(&mut lines, &rest[start..], depth, &mut budget)?;
            break;
        };
        match tag_kind(tag) {
            TagKind::Declaration => push_line(&mut lines, tag, depth, &mut budget)?,
            TagKind::Closing => {
                depth = depth.saturating_sub(1);
                push_line(&mut lines, tag, depth, &mut budget)?;
            }
            TagKind::SelfClosing => push_line(&mut lines, tag, depth, &mut budget)?,
            TagKind::Opening { name } => {
                push_line(&mut lines, tag, depth, &mut budget)?;
                if !void_elements
                    .iter()
                    .any(|void| name.eq_ignore_ascii_case(void))
                {
                    depth += 1;
                }
            }
        }
        rest = remainder;
    }
    Ok(lines.join("\n"))
}

/// One markup tag and what is left after it. `rest` starts at `<`.
fn take_tag(rest: &str) -> Option<(&str, &str)> {
    let end = if rest.starts_with("<!--") {
        rest.find("-->")?.checked_add(3)?
    } else if rest.starts_with("<![CDATA[") {
        rest.find("]]>")?.checked_add(3)?
    } else {
        rest.find('>')?.checked_add(1)?
    };
    Some((&rest[..end], rest.get(end..)?))
}

/// What a tag does to the indentation depth.
enum TagKind<'a> {
    /// `<?…?>`, `<!…>`, a comment or a CDATA block: shown, but never opens a level.
    Declaration,
    /// `</…>`: closes a level before it is shown.
    Closing,
    /// `<… />`: shown without opening a level.
    SelfClosing,
    /// `<…>`: shown and, unless a void element, opens a level.
    Opening { name: &'a str },
}

/// Classify one tag (with its brackets) for [`indent_markup`].
fn tag_kind(tag: &str) -> TagKind<'_> {
    let inner = tag
        .strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
        .unwrap_or(tag);
    match inner.as_bytes().first() {
        Some(b'?') | Some(b'!') => TagKind::Declaration,
        Some(b'/') => TagKind::Closing,
        _ if inner.trim_end().ends_with('/') => TagKind::SelfClosing,
        _ => TagKind::Opening {
            name: inner
                .split(|c: char| c.is_ascii_whitespace() || c == '/')
                .next()
                .unwrap_or(inner),
        },
    }
}

/// One output line at `depth`: the content — a tag or a text run — on a single line,
/// indented no deeper than [`MAX_INDENT_DEPTH`]. `budget` is what the output may still
/// grow by; the line takes its size from it or the format is refused as
/// [`FormatError::TooLarge`].
fn push_line(
    lines: &mut Vec<String>,
    text: &str,
    depth: usize,
    budget: &mut usize,
) -> Result<(), FormatError> {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return Ok(());
    }
    let indent = "  ".repeat(depth.min(MAX_INDENT_DEPTH));
    *budget = budget
        .checked_sub(indent.len() + collapsed.len() + 1)
        .ok_or(FormatError::TooLarge)?;
    lines.push(format!("{indent}{collapsed}"));
    Ok(())
}

#[cfg(test)]
#[path = "pretty_tests.rs"]
mod tests;
