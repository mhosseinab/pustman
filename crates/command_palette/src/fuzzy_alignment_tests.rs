//! The scorer grades the *best* alignment of the query in a candidate, not the greedy
//! leftmost one: a tight match later in the candidate outranks a loose earlier one.

use super::rank;

/// The ranks of `matches` as candidate indices.
fn order_of(matches: &[super::StringMatch]) -> Vec<usize> {
    matches
        .iter()
        .map(|string_match| string_match.candidate_index)
        .collect()
}

#[test]
fn fuzzy_tight_later_match_outranks_loose_earlier_match() {
    let matches = rank("ab", ["axb_ab", "zab"]);

    assert_eq!(
        order_of(&matches),
        vec![0, 1],
        "\"axb_ab\" matches at the word start of \"_ab\", a contiguous pair that \
         outranks the bare contiguous run \"zab\"; the greedy leftmost alignment \
         [0, 2] scattered inside \"axb_ab\" used to rank it lower"
    );
}

#[test]
fn fuzzy_reports_the_positions_of_the_best_alignment() {
    let matches = rank("ab", ["axb_ab"]);

    assert_eq!(
        matches[0].positions,
        vec![4, 5],
        "the highlighted characters are the tightest alignment, not the leftmost one"
    );
}

#[test]
fn fuzzy_word_start_later_in_the_candidate_beats_earlier_scattered_hit() {
    let matches = rank("ab", ["aXb_b"]);

    assert_eq!(
        matches[0].positions,
        vec![0, 4],
        "\"b\" at the word start of \"_b\" beats the earlier \"b\" after \"X\", \
         so the best alignment scores the later one"
    );
}
