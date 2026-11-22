use gpui::SharedString;

use super::{JsonTarget, js_string_literal, json_string_literal, json_target_at, property_path};
use crate::body_selection::BodyPoint;
use crate::json_syntax::{LineSpans, json_line_spans};

/// `{"data":[{"name":"Ann \"A\"","age":3}],"ok":true,"a-b":null}`, pretty-printed.
const PRETTY: &str = r#"{
  "data": [
    {
      "name": "Ann \"A\"",
      "age": 3
    }
  ],
  "ok": true,
  "a-b": null
}"#;

fn lines() -> (Vec<SharedString>, Vec<LineSpans>) {
    let lines = PRETTY
        .split('\n')
        .map(SharedString::from)
        .collect::<Vec<_>>();
    let spans = lines.iter().map(|line| json_line_spans(line)).collect();
    (lines, spans)
}

/// The target under the first occurrence of `needle` on line `line`.
fn target(line: usize, needle: &str) -> JsonTarget {
    let (lines, spans) = lines();
    let column = lines
        .get(line)
        .and_then(|text| text.find(needle))
        .unwrap_or_else(|| panic!("line {line} holds {needle:?}"));
    json_target_at(&lines, &spans, BodyPoint { line, column })
        .unwrap_or_else(|| panic!("a JSON target under {needle:?}"))
}

#[test]
fn test_a_clicked_key_names_its_path_value_and_string() {
    let target = target(3, "\"name\"");
    assert_eq!(property_path(&target.path), "data[0].name");
    assert_eq!(target.value, r#"Ann "A""#);
    assert_eq!(target.string.as_deref(), Some("name"));
}

#[test]
fn test_a_clicked_string_value_copies_its_unescaped_contents() {
    let target = target(3, "\"Ann");
    assert_eq!(property_path(&target.path), "data[0].name");
    assert_eq!(target.value, r#"Ann "A""#);
    assert_eq!(target.string.as_deref(), Some(r#"Ann "A""#));
}

#[test]
fn test_a_clicked_number_has_a_value_but_no_string() {
    let target = target(4, "3");
    assert_eq!(property_path(&target.path), "data[0].age");
    assert_eq!(target.value, "3");
    assert_eq!(target.string, None);
}

#[test]
fn test_a_clicked_container_copies_its_whole_text_dedented() {
    let target = target(1, "[");
    assert_eq!(property_path(&target.path), "data");
    assert_eq!(
        target.value,
        "[\n  {\n    \"name\": \"Ann \\\"A\\\"\",\n    \"age\": 3\n  }\n]"
    );
}

#[test]
fn test_a_click_in_the_indentation_targets_the_lines_first_token() {
    let target = target(7, " ");
    assert_eq!(property_path(&target.path), "ok");
    assert_eq!(target.value, "true");
}

#[test]
fn test_a_key_that_is_not_an_identifier_is_bracketed_in_the_path() {
    let target = target(8, "\"a-b\"");
    assert_eq!(property_path(&target.path), r#"["a-b"]"#);
    assert_eq!(target.value, "null");
}

#[test]
fn test_the_root_object_has_an_empty_path_and_the_whole_body_as_value() {
    let target = target(0, "{");
    assert_eq!(property_path(&target.path), "");
    assert_eq!(target.value, PRETTY);
}

#[test]
fn test_string_literals_escape_for_their_language() {
    let text = "it's \"q\"\n";
    assert_eq!(json_string_literal(text), r#""it's \"q\"\n""#);
    assert_eq!(js_string_literal(text), r#"'it\'s "q"\n'"#);
}
