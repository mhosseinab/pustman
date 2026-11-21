//! Pretty JSON is coloured by what each piece is: keys, strings, numbers, booleans,
//! null and punctuation each get their own span, and find matches layer over them.

use gpui::{HighlightStyle, hsla};

use super::JsonToken::{Boolean, Key, Null, Number, Punctuation, String};
use super::{json_line_spans, layer_highlights};

#[test]
fn test_a_key_and_its_string_value_are_told_apart() {
    let line = r#"  "name": "pustman","#;
    assert_eq!(
        json_line_spans(line),
        vec![
            (2..8, Key),
            (8..9, Punctuation),
            (10..19, String),
            (19..20, Punctuation)
        ]
    );
}

#[test]
fn test_numbers_booleans_and_null_get_their_own_spans() {
    assert_eq!(
        json_line_spans(r#"  "n": -1.5e3,"#),
        vec![
            (2..5, Key),
            (5..6, Punctuation),
            (7..13, Number),
            (13..14, Punctuation)
        ]
    );
    assert_eq!(
        json_line_spans("    true,"),
        vec![(4..8, Boolean), (8..9, Punctuation)]
    );
    assert_eq!(json_line_spans("false"), vec![(0..5, Boolean)]);
    assert_eq!(json_line_spans("  null"), vec![(2..6, Null)]);
}

#[test]
fn test_brackets_are_punctuation() {
    assert_eq!(
        json_line_spans(r#"  "tags": ["#),
        vec![(2..8, Key), (8..9, Punctuation), (10..11, Punctuation)]
    );
    assert_eq!(
        json_line_spans("  ],"),
        vec![(2..3, Punctuation), (3..4, Punctuation)]
    );
    assert_eq!(
        json_line_spans("{}"),
        vec![(0..1, Punctuation), (1..2, Punctuation)]
    );
}

#[test]
fn test_an_escaped_quote_does_not_end_a_string() {
    let line = r#""a\"b": "c\\""#;
    assert_eq!(
        json_line_spans(line),
        vec![(0..6, Key), (6..7, Punctuation), (8..13, String)]
    );
}

#[test]
fn test_a_string_that_looks_like_a_key_inside_a_value_is_a_string() {
    // The colon is inside the string, so the string is a value, not a key.
    assert_eq!(json_line_spans(r#"  "a: b""#), vec![(2..8, String)]);
}

#[test]
fn test_multibyte_text_keeps_spans_on_char_boundaries() {
    let line = r#""é": "ü""#;
    let spans = json_line_spans(line);
    assert_eq!(
        spans,
        vec![(0..4, Key), (4..5, Punctuation), (6..10, String)]
    );
    for (range, _) in spans {
        assert!(line.is_char_boundary(range.start) && line.is_char_boundary(range.end));
    }
}

#[test]
fn test_an_unterminated_string_runs_to_the_end_of_the_line() {
    assert_eq!(json_line_spans(r#"  "abc"#), vec![(2..6, String)]);
}

fn colour(hue: f32) -> HighlightStyle {
    HighlightStyle {
        color: Some(hsla(hue, 0.5, 0.5, 1.)),
        ..HighlightStyle::default()
    }
}

fn background() -> HighlightStyle {
    HighlightStyle {
        background_color: Some(hsla(0.1, 0.9, 0.5, 1.)),
        ..HighlightStyle::default()
    }
}

#[test]
fn test_a_find_match_layers_over_syntax_colour_without_overlapping_runs() {
    let key = colour(0.2);
    let string = colour(0.4);
    let base = vec![(0..6, key), (8..13, string)];
    // The match starts inside the key and ends inside the string.
    let overlay = vec![(4..10, background())];

    let layered = layer_highlights(&base, &overlay);

    let both = |style: HighlightStyle| style.highlight(background());
    assert_eq!(
        layered,
        vec![
            (0..4, key),
            (4..6, both(key)),
            (6..8, background()),
            (8..10, both(string)),
            (10..13, string),
        ]
    );
}

#[test]
fn test_no_overlay_leaves_the_syntax_colours_as_they_are() {
    let base = vec![(0..3, colour(0.2)), (5..7, colour(0.3))];
    assert_eq!(layer_highlights(&base, &[]), base);
}
