//! The URL rebuild under the Params tab's query rows (#139): a row that says nothing
//! about the request — no key and no value, the empty row `KeyValueTable::add_row`
//! seeds and what an empty `&`-segment of a typed URL parses to — must not decorate
//! the URL with a trailing `&` or a bare `?` once another row is edited.
//!
//! `Decision:` such rows are omitted from the URL and kept in the table, exactly like
//! disabled rows (S03/R03: "Validation never silently rewrites the intended payload" —
//! an empty segment carries no payload), so a typed `?a=1&&b=2` rebuilds as `?a=1&b=2`
//! and an untouched `add_row` row leaves the URL alone. A key-only row (`?a`) is a
//! payload and still round-trips.

use api_model::KeyValue;

use super::{build_url, parse_query};

/// One query row with no description and nothing imported in `extra`.
fn entry(key: &str, value: Option<&str>) -> KeyValue {
    KeyValue {
        key: String::from(key),
        value: value.map(String::from),
        description: None,
        enabled: true,
        extra: Default::default(),
    }
}

/// The row `add_row` seeds: no key, no value — it says nothing about the request.
fn empty_row() -> KeyValue {
    entry("", None)
}

#[test]
fn test_an_unedited_empty_row_adds_no_trailing_ampersand_to_the_url() {
    let rows = vec![entry("q", Some("1")), empty_row()];
    assert_eq!(
        build_url("https://example.com/search", &rows, None),
        "https://example.com/search?q=1",
        "the unedited empty row contributes no trailing & to the rebuilt URL"
    );
}

#[test]
fn test_an_unedited_empty_row_leaves_no_bare_question_mark() {
    let rows = vec![empty_row()];
    assert_eq!(
        build_url("https://example.com/search", &rows, None),
        "https://example.com/search",
        "a table holding only the empty row builds a URL with no query at all"
    );
}

#[test]
fn test_an_empty_row_between_two_rows_adds_no_empty_segment() {
    let rows = vec![entry("a", Some("1")), empty_row(), entry("b", Some("2"))];
    assert_eq!(
        build_url("https://example.com/search", &rows, None),
        "https://example.com/search?a=1&b=2",
        "an empty row in the middle joins no empty segment into the URL either"
    );
}

#[test]
fn test_an_empty_query_segment_says_nothing_and_is_dropped_from_the_url() {
    let rows = parse_query("a=1&&b=2");
    assert_eq!(
        build_url("https://example.com/search", &rows, None),
        "https://example.com/search?a=1&b=2",
        "the empty segment of a typed URL is an empty row, and an empty row builds nothing"
    );
}

#[test]
fn test_a_key_only_row_still_round_trips_into_the_url() {
    let rows = parse_query("a&b=2");
    assert_eq!(
        build_url("https://example.com/search", &rows, None),
        "https://example.com/search?a&b=2",
        "a key-only row is a payload and round-trips verbatim"
    );
}

#[test]
fn test_the_fragment_sits_after_the_query_the_rows_rebuild() {
    let rows = vec![entry("q", Some("1")), empty_row()];
    assert_eq!(
        build_url("https://example.com/search", &rows, Some("top")),
        "https://example.com/search?q=1#top",
        "the fragment tail rides along after the rebuilt query"
    );
}
