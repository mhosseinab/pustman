//! The units a multi-click selects: a double click takes the run of like characters
//! under the pointer (a word, a gap of whitespace or a run of punctuation), a triple
//! click the line it is on, with its line break.

use super::{line_range_at, word_range_at};

#[test]
fn test_a_double_click_inside_a_word_selects_the_whole_word() {
    let text = "GET https://api.example.com/users_list?id=7";
    let inside_users = text
        .find("users")
        .map(|start| start + 2)
        .unwrap_or_default();
    assert_eq!(
        &text[word_range_at(text, inside_users)],
        "users_list",
        "underscores join a word, as in an identifier"
    );
}

#[test]
fn test_a_double_click_on_punctuation_selects_the_run_of_punctuation() {
    let text = "a://b";
    assert_eq!(&text[word_range_at(text, 1)], "://");
}

#[test]
fn test_a_double_click_on_whitespace_selects_the_gap() {
    let text = "one   two";
    assert_eq!(word_range_at(text, 4), 3..6);
}

#[test]
fn test_a_double_click_past_the_end_selects_the_last_word() {
    let text = "say hello";
    assert_eq!(&text[word_range_at(text, text.len())], "hello");
}

#[test]
fn test_a_double_click_keeps_multibyte_words_whole() {
    let text = "naïve café";
    assert_eq!(&text[word_range_at(text, 2)], "naïve");
    assert_eq!(word_range_at("", 0), 0..0);
}

#[test]
fn test_a_triple_click_selects_the_line_with_its_line_break() {
    let text = "first\nsecond\nthird";
    assert_eq!(&text[line_range_at(text, 8)], "second\n");
    assert_eq!(&text[line_range_at(text, text.len())], "third");
    assert_eq!(&text[line_range_at(text, 0)], "first\n");
}
