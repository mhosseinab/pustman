//! The scorer ranks the way a user means a query: an exact prefix first, then word
//! starts, then contiguous runs, then scattered letter hits — and everything the query
//! cannot spell at all is not a match. Positions are char indices, so multi-byte
//! candidates neither panic nor report byte offsets.

use super::{rank, score};

/// The ranks of `matches` as candidate indices.
fn order_of(matches: &[super::StringMatch]) -> Vec<usize> {
    matches
        .iter()
        .map(|string_match| string_match.candidate_index)
        .collect()
}

#[test]
fn fuzzy_query_ranks_prefix_first() {
    let matches = rank("ab", ["azb", "zab", "alpha_bravo", "abacus"]);

    assert_eq!(
        order_of(&matches),
        vec![3, 2, 1, 0],
        "the exact prefix \"abacus\" outranks the word-start match \"alpha_bravo\", \
         which outranks the contiguous run \"zab\", which outranks the scattered \"azb\""
    );
    assert_eq!(
        matches[0].positions,
        vec![0, 1],
        "the prefix match highlights its first two characters"
    );
}

#[test]
fn fuzzy_word_start_matches_beat_scattered_matches() {
    let matches = rank("ab", ["azzb", "alpha_bravo"]);

    assert_eq!(
        order_of(&matches),
        vec![1, 0],
        "\"alpha_bravo\" matches at the \"b\" of \"bravo\", a word start; \
         \"azzb\" only scatters the letters"
    );
}

#[test]
fn fuzzy_excludes_candidates_that_are_not_subsequences() {
    let matches = rank("ab", ["ab", "ba", "abc"]);

    assert_eq!(
        order_of(&matches),
        vec![0, 2],
        "\"ba\" never spells \"ab\" in order and is not a match"
    );
}

#[test]
fn fuzzy_matching_ignores_case() {
    assert_eq!(
        score("AB", "abacus"),
        score("ab", "ABACUS"),
        "the query and the candidate match case-insensitively, with the same grade"
    );
    assert_eq!(
        order_of(&rank("aB", ["zzz", "Alpha_Bravo"])),
        vec![1],
        "a mixed-case query still finds the candidate"
    );
}

#[test]
fn fuzzy_empty_query_keeps_the_original_order() {
    let matches = rank("", ["zeta", "alpha", "mid"]);

    assert_eq!(
        order_of(&matches),
        vec![0, 1, 2],
        "an empty query matches everything and ranks nothing"
    );
}

#[test]
fn fuzzy_multi_byte_candidates_report_char_indices() {
    let matches = rank("ve", ["naïve-test"]);

    assert_eq!(
        matches[0].positions,
        vec![3, 4],
        "positions count characters, so \"ï\" does not shift them by its extra byte"
    );

    let emoji = rank("☕", ["café ☕"]);
    assert_eq!(
        emoji[0].positions,
        vec![5],
        "the cup is the sixth character, however many bytes it takes"
    );
}
