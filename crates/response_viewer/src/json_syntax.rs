//! JSON syntax colouring for Pretty mode. The spans are computed per formatted line when
//! the body is formatted, off the foreground, and cached beside the lines; render only
//! looks them up for the visible rows.

use std::ops::Range;

use gpui::HighlightStyle;

/// What a span of a JSON line is, which picks its theme colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JsonToken {
    Key,
    String,
    Number,
    Boolean,
    Null,
    Punctuation,
}

/// One line's spans: byte ranges in order, never overlapping.
pub(crate) type LineSpans = Vec<(Range<usize>, JsonToken)>;

/// The spans of one line of pretty-printed JSON.
/// The formatter puts at most one key and one value on a line, and a JSON string never
/// holds a raw newline, so every token starts and ends on the line it begins on.
pub(crate) fn json_line_spans(line: &str) -> LineSpans {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let (end, token) = match byte {
            b'"' => {
                let end = string_end(bytes, index);
                // A string followed by a colon is the key of the pair it starts.
                let token = match bytes
                    .get(end..)
                    .unwrap_or_default()
                    .iter()
                    .find(|b| !b.is_ascii_whitespace())
                {
                    Some(b':') => JsonToken::Key,
                    _ => JsonToken::String,
                };
                (end, Some(token))
            }
            b'{' | b'}' | b'[' | b']' | b',' | b':' => (index + 1, Some(JsonToken::Punctuation)),
            b'-' | b'0'..=b'9' => (
                scan(bytes, index, |b| {
                    b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E')
                }),
                Some(JsonToken::Number),
            ),
            b'a'..=b'z' => {
                let end = scan(bytes, index, |b| b.is_ascii_lowercase());
                let token = match &bytes[index..end] {
                    b"true" | b"false" => Some(JsonToken::Boolean),
                    b"null" => Some(JsonToken::Null),
                    _ => None,
                };
                (end, token)
            }
            _ => (index + 1, None),
        };
        if let Some(token) = token {
            spans.push((index..end, token));
        }
        index = end;
    }
    spans
}

/// Where the string opening at `start` ends: just past its closing quote, or the end of
/// the line when it is not closed there.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 1;
    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

/// The end of the run of bytes from `start` that `belongs` accepts.
fn scan(bytes: &[u8], start: usize, belongs: impl Fn(u8) -> bool) -> usize {
    bytes
        .get(start..)
        .unwrap_or_default()
        .iter()
        .position(|&byte| !belongs(byte))
        .map_or(bytes.len(), |offset| start + offset)
}

/// Lay `overlay` (find matches) over `base` (syntax colours): the result is sorted and
/// non-overlapping, as `StyledText::with_highlights` requires, and where both apply the
/// overlay's fields win.
pub(crate) fn layer_highlights(
    base: &[(Range<usize>, HighlightStyle)],
    overlay: &[(Range<usize>, HighlightStyle)],
) -> Vec<(Range<usize>, HighlightStyle)> {
    if overlay.is_empty() {
        return base.to_vec();
    }
    let mut bounds = base
        .iter()
        .chain(overlay)
        .flat_map(|(range, _)| [range.start, range.end])
        .collect::<Vec<_>>();
    bounds.sort_unstable();
    bounds.dedup();
    let covering = |spans: &[(Range<usize>, HighlightStyle)], at: usize| {
        spans
            .iter()
            .find(|(range, _)| range.contains(&at))
            .map(|(_, style)| *style)
    };
    bounds
        .windows(2)
        .filter_map(|window| {
            let &[start, end] = window else {
                return None;
            };
            let style = match (covering(base, start), covering(overlay, start)) {
                (Some(below), Some(above)) => below.highlight(above),
                (Some(only), None) | (None, Some(only)) => only,
                (None, None) => return None,
            };
            Some((start..end, style))
        })
        .collect()
}

#[cfg(test)]
#[path = "json_syntax_tests.rs"]
mod tests;
