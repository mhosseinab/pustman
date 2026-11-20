//! The Preview view (REQ-RESP-02): images render from the bytes the response carried,
//! and HTML shows as its visible text.
//!
//! Preview is deliberately not a browser. It never runs a script and never makes a
//! request: an image is decoded from the body's own bytes by GPUI's local decoder — never
//! `ImageSource::Resource`, which fetches — and an HTML body is reduced to plain text by
//! [`html_text`], with a note under it saying exactly that.

use std::sync::Arc;

use gpui::{
    AnyElement, App, ElementId, Image, ImageFormat, ImageSource, IntoElement, ObjectFit,
    RenderOnce, Window, img,
};
use ui::prelude::*;

use crate::body_view::{note, render_line};
use crate::child_id;
use crate::pretty::Formatter;

/// What the note under an HTML preview says: why the body is text only, and what Preview
/// never does (PRD REQ-RESP-02).
pub const TEXT_ONLY_NOTE: &str =
    "Preview shows text only — scripts and remote content are never loaded";

/// What Preview draws for a body (REQ-RESP-02).
#[derive(Clone, Debug)]
pub enum PreviewContent {
    /// An image body: decoded from the bytes it arrived with.
    Image(Arc<Image>),
    /// An HTML body: its visible text, and the note that explains what is missing.
    Text {
        lines: Vec<SharedString>,
        note: SharedString,
    },
    /// A body Preview cannot show — binary, or a type only Pretty, Raw and Hex cover.
    Unsupported,
}

impl PreviewContent {
    /// What Preview shows for a body: the image `Content-Type` names, the visible text of
    /// an HTML document, or [`Self::Unsupported`]. Pure — the body view runs it off the
    /// foreground, the way it formats bodies.
    pub fn for_body(content_type: Option<&str>, bytes: &[u8]) -> Self {
        if let Some(format) = image_format(content_type) {
            return Self::Image(Arc::new(Image::from_bytes(format, bytes.to_vec())));
        }
        if Formatter::detect(content_type) == Formatter::Html {
            let source = String::from_utf8_lossy(bytes);
            let text = html_text(&source);
            return Self::Text {
                lines: text.lines().map(SharedString::from).collect(),
                note: SharedString::new_static(TEXT_ONLY_NOTE),
            };
        }
        Self::Unsupported
    }
}

/// The image format `Content-Type` names, for every format GPUI decodes from bytes. SVG
/// is excluded on purpose: a vector image can reference external resources and script,
/// and Preview loads neither.
fn image_format(content_type: Option<&str>) -> Option<ImageFormat> {
    let parsed = content_type?.parse::<mime::Mime>().ok()?;
    if parsed.type_() != mime::IMAGE {
        return None;
    }
    match ImageFormat::from_mime_type(parsed.essence_str()) {
        Some(ImageFormat::Svg) | None => None,
        Some(format) => Some(format),
    }
}

/// The visible text of an HTML document: every tag, attribute, comment, script and style
/// dropped, the entities that stand for characters decoded. Small on purpose — this is a
/// text fallback, not a web engine (PRD REQ-RESP-02).
fn html_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        push_text(&mut text, &rest[..open]);
        let at_tag = &rest[open..];
        match skip_construct(at_tag) {
            Some((after, block)) => {
                if block {
                    text.push('\n');
                }
                rest = after;
            }
            None => {
                // A literal '<' that opens no tag is text like any other.
                text.push('<');
                rest = &at_tag[1..];
            }
        }
    }
    push_text(&mut text, rest);
    normalize_whitespace(&text)
}

/// Skip the markup at the front of `tag` (which starts with `<`): a comment, a
/// declaration or one tag — plus the whole element when the tag opens a script, style or
/// title, whose content is code or metadata, never visible text. Returns what follows
/// and whether the markup breaks the visible line. [`None`] means the `<` opened no
/// markup at all.
fn skip_construct(tag: &str) -> Option<(&str, bool)> {
    let body = tag.strip_prefix('<')?;
    if let Some(after) = body.strip_prefix("!--") {
        return Some((find_after(after, "-->").unwrap_or(""), false));
    }
    if body.starts_with('!') || body.starts_with('?') {
        return Some((find_after(body, ">").unwrap_or(""), false));
    }
    let (closing, name_start) = match body.strip_prefix('/') {
        Some(after) => (true, after),
        None => (false, body),
    };
    let (name, after_name) = read_tag_name(name_start)?;
    let after_tag = skip_attributes(after_name)?;
    if !closing
        && ["script", "style", "title"]
            .iter()
            .any(|dropped| name.eq_ignore_ascii_case(dropped))
    {
        return Some((skip_closing_element(after_tag, name), true));
    }
    Some((after_tag, is_block_tag(name)))
}

/// The tag name at the front of `body` and what follows it. Names are ASCII
/// alphanumeric, starting with a letter.
fn read_tag_name(body: &str) -> Option<(&str, &str)> {
    let end = body
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(body.len());
    let name = body.get(..end)?;
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    Some((name, &body[end..]))
}

/// What follows the attributes of a tag — past its `>`, never stopping at one inside a
/// quoted attribute value. [`None`] when the tag never closes.
fn skip_attributes(rest: &str) -> Option<&str> {
    let mut quote = None;
    for (offset, character) in rest.char_indices() {
        match quote {
            Some(open) => {
                if character == open {
                    quote = None;
                }
            }
            None => match character {
                '"' | '\'' => quote = Some(character),
                '>' => return rest.get(offset + 1..),
                _ => {}
            },
        }
    }
    None
}

/// What follows the `</name …>` that closes an element whose content is dropped — or the
/// end of the document when no closing tag exists.
fn skip_closing_element<'a>(rest: &'a str, name: &str) -> &'a str {
    let closing = format!("</{name}");
    let mut scan = rest;
    while let Some(open) = scan.find('<') {
        let at_tag = &scan[open..];
        if starts_with_ignore_ascii_case(at_tag, &closing)
            && let Some(after) = at_tag.get(closing.len()..).and_then(skip_attributes)
        {
            return after;
        }
        scan = &at_tag[1..];
    }
    rest
}

/// Whether `haystack` starts with `needle`, letters compared case-insensitively.
fn starts_with_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .get(..needle.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(needle.as_bytes()))
}

/// The text after the next `marker`, or [`None`] when the document runs out first.
fn find_after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    text.find(marker)
        .and_then(|offset| text.get(offset + marker.len()..))
}

/// Whether the tag breaks the visible line the way a block element does. Tag names are
/// compared case-insensitively, the way HTML reads them.
fn is_block_tag(name: &str) -> bool {
    [
        "br", "p", "div", "li", "tr", "h1", "h2", "h3", "h4", "h5", "h6",
    ]
    .iter()
    .any(|block| name.eq_ignore_ascii_case(block))
}

/// Copy a run of visible text into `text`, decoding the entities that stand for
/// characters.
fn push_text(text: &mut String, run: &str) {
    let mut rest = run;
    while let Some(start) = rest.find('&') {
        text.push_str(&rest[..start]);
        let at_entity = &rest[start..];
        match decode_entity(at_entity) {
            Some((decoded, after)) => {
                text.push(decoded);
                rest = after;
            }
            None => {
                text.push('&');
                rest = &at_entity[1..];
            }
        }
    }
    text.push_str(rest);
}

/// The character an entity stands for and the text after it. Entities that are not one
/// of the named five (or a numeric reference) stay literal text.
fn decode_entity(entity: &str) -> Option<(char, &str)> {
    let end = entity.find(';')?;
    let name = entity.get(1..end)?;
    let after = entity.get(end + 1..)?;
    let decoded = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let digits = name
                .strip_prefix("&#x")
                .or_else(|| name.strip_prefix("&#X"));
            match digits {
                Some(hex_digits) => char::from_u32(u32::from_str_radix(hex_digits, 16).ok()?)?,
                None => char::from_u32(name.strip_prefix("&#")?.parse().ok()?)?,
            }
        }
    };
    Some((decoded, after))
}

/// Trim every line, collapse its whitespace runs the way a browser would, and drop the
/// lines the markup left empty.
fn normalize_whitespace(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    for line in text.lines() {
        let mut collapsed = line.split_whitespace().peekable();
        if collapsed.peek().is_none() {
            continue;
        }
        if !normalized.is_empty() {
            normalized.push('\n');
        }
        for (position, word) in collapsed.enumerate() {
            if position > 0 {
                normalized.push(' ');
            }
            normalized.push_str(word);
        }
    }
    normalized
}

/// The Preview pane: the image a body carries, or the text it can safely show.
#[derive(IntoElement)]
pub struct PreviewPane {
    base: ElementId,
    content: PreviewContent,
}

impl PreviewPane {
    /// The pane for `content`, elements keyed under `base`.
    pub fn new(base: impl Into<ElementId>, content: PreviewContent) -> Self {
        Self {
            base: base.into(),
            content,
        }
    }
}

impl RenderOnce for PreviewPane {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        match self.content {
            PreviewContent::Image(image) => render_image(&self.base, image, cx),
            PreviewContent::Text { lines, note: why } => {
                render_text(&self.base, lines, why, cx).into_any_element()
            }
            PreviewContent::Unsupported => note(
                "Preview shows images and HTML — open Pretty, Raw or Hex for this body.",
                cx,
            )
            .into_any_element(),
        }
    }
}

/// An image body: the bytes decoded in place, with a note instead of a blank pane when
/// they will not decode.
fn render_image(base: &ElementId, image: Arc<Image>, cx: &mut App) -> AnyElement {
    let padding = DynamicSpacing::Base02.rems(cx);
    v_flex()
        .size_full()
        .px(DynamicSpacing::Base04.rems(cx))
        .py(padding)
        .child(
            img(ImageSource::Image(image))
                .id(child_id(base, "image"))
                .max_w_full()
                .object_fit(ObjectFit::Contain)
                .with_loading(move || {
                    div()
                        .px(padding)
                        .py(padding)
                        .child(Label::new("Loading image…").color(Color::Muted))
                        .into_any_element()
                })
                .with_fallback(move || {
                    div()
                        .px(padding)
                        .py(padding)
                        .debug_selector(|| "preview-image-fallback".to_owned())
                        .child(Label::new("Could not decode this image.").color(Color::Error))
                        .into_any_element()
                }),
        )
        .into_any_element()
}

/// An HTML body: its visible text, one line per line like the Raw lines, with the note
/// that says why the rest is missing.
fn render_text(
    base: &ElementId,
    lines: Vec<SharedString>,
    why: SharedString,
    cx: &mut App,
) -> impl IntoElement {
    let mut pane = v_flex().size_full();
    if lines.is_empty() {
        pane = pane.child(note("No visible text in this document.", cx));
    } else {
        let row_lines = Arc::new(lines);
        let row_base = base.clone();
        let list_base = base.clone();
        let render_base = base.clone();
        pane = pane.child(VirtualList::new(
            child_id(&list_base, "text-lines"),
            row_lines.len(),
            move |index| child_id(&row_base, format!("line-{index}")),
            move |index, _window, cx| render_line(&row_lines, index, &render_base, cx),
        ));
    }
    pane.child(note(why, cx))
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
