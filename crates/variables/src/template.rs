//! The `{{name}}` placeholder syntax, shared by [`crate::resolve`] and [`crate::spans`].

use std::ops::Range;

/// One `{{name}}` placeholder in a template.
pub(crate) struct Placeholder<'a> {
    /// The name between the braces, verbatim: `api_key`, or `$guid` for a dynamic
    /// variable.
    pub(crate) name: &'a str,
    /// Byte range of the whole `{{name}}` token, braces included.
    pub(crate) range: Range<usize>,
}

/// Every placeholder of `text`, left to right.
///
/// A name is the text between a `{{` and the `}}` that follows it; it must be non-empty
/// and hold no braces, so `{{a{{b}}` holds only `{{b}}`. Text that has no name to
/// resolve — an unbalanced `{{`, an empty `{{}}` — stays ordinary text.
pub(crate) fn placeholders(text: &str) -> Vec<Placeholder<'_>> {
    let mut found = Vec::new();
    let mut cursor = 0;
    while let Some(open_offset) = text[cursor..].find("{{") {
        let open = cursor + open_offset;
        let name_start = open + 2;
        let Some(close_offset) = text[name_start..].find("}}") else {
            break;
        };
        let close = name_start + close_offset;
        let name = &text[name_start..close];
        if name.is_empty() || name.contains(['{', '}']) {
            // Resuming after the opening braces keeps the rest of the text in play.
            cursor = name_start;
            continue;
        }
        found.push(Placeholder {
            name,
            range: open..close + 2,
        });
        cursor = close + 2;
    }
    found
}
