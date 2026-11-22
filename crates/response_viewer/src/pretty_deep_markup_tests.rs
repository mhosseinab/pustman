//! Deep but well-formed markup formats in Pretty mode (#137): the output budget bounds
//! hostile bodies without refusing documents that are simply deeply nested.

use super::Formatter;

/// A well-formed document nested twenty deep formats. Depth — not size — makes the
/// indentation grow, so a body like this formats to roughly 8× its input; the budget
/// must leave room for it instead of refusing it and falling back to Raw.
#[test]
fn well_formed_markup_at_depth_twenty_formats() {
    let mut body = String::new();
    for _ in 0..20 {
        body.push_str("<a>");
    }
    body.push_str("Tove");
    for _ in 0..20 {
        body.push_str("</a>");
    }

    let formatted = match Formatter::Xml.format(body.as_bytes()) {
        Ok(formatted) => formatted,
        Err(error) => panic!("a well-formed depth-20 document must format, got {error}"),
    };
    let lines: Vec<&str> = formatted.lines().collect();
    assert_eq!(lines.len(), 41, "one line per tag, plus the text line");
    assert_eq!(lines[0], "<a>", "the outermost tag opens the document");
    assert_eq!(
        lines[20],
        format!("{}Tove", " ".repeat(40)),
        "the deepest line is indented two spaces per level"
    );
}
