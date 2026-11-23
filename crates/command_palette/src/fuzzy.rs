//! The fuzzy scorer the [`crate::Picker`] ranks its matches with.
//!
//! A simple scorer on purpose: `score` grades the best alignment of the query in the
//! candidate — the strongest way to spell the query out as a case-insensitive
//! subsequence — by where it landed: an exact prefix
//! first, then matches on word starts (after ` `, `_`, `-`, `/`, `.`, `:` or a camelCase
//! hump), then contiguous runs, then scattered letter hits. Zed's `fuzzy` crate is GPL
//! and is never copied, and no third-party matcher (`fuzzy-matcher`, `nucleo`, …) is
//! needed for a query box.

/// How strong a match is. Bigger ranks first; see the tier weights in [`score`].
pub type Score = i32;

/// One candidate's match: where it landed in the candidate list, how strong it is, and
/// which characters matched — char indices, so a view can highlight them later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringMatch {
    /// Index of the candidate in the slice [`rank`] was given.
    pub candidate_index: usize,
    /// How strong the match is, bigger first.
    pub score: Score,
    /// The matched characters' positions, as char indices into the candidate.
    pub positions: Vec<usize>,
}

/// The ranking tiers. Each tier sits an order of magnitude above the one below it, so
/// the bonuses can reorder matches *within* a tier but never across tiers (they are
/// bounded by `73 × query length`, well under the gaps for any realistic query).
///
/// `Decision:` these are the weights: tiers `1_000_000` (exact prefix) > `100_000`
/// (a word-start match) > `10_000` (a contiguous run) > `0` (scattered), plus `64` per
/// character matched at a word start, `8` per adjacent matched pair and `1` per matched
/// character.
const TIER_EXACT_PREFIX: Score = 1_000_000;
const TIER_WORD_START: Score = 100_000;
const TIER_CONTIGUOUS: Score = 10_000;
const WORD_START_BONUS: Score = 64;
const CONTIGUITY_BONUS: Score = 8;
const CHAR_BONUS: Score = 1;

/// A character a word starts after, on top of the camelCase hump
/// (`previous.is_lowercase() && current.is_uppercase()`).
const WORD_START_AFTER: [char; 6] = [' ', '_', '-', '/', '.', ':'];

/// How strong `query`'s match against `candidate` is, or [`None`] when the query is not
/// a case-insensitive subsequence of the candidate. An empty query matches everything
/// with a score of 0 — no evidence, no ranking.
pub fn score(query: &str, candidate: &str) -> Option<Score> {
    match_candidate(query, candidate).map(|(score, _positions)| score)
}

/// Rank `candidates` against `query`: the strongest match first, ties keeping the
/// candidates' original order. Candidates the query does not match are left out; an
/// empty query returns every candidate in its original order.
pub fn rank<'a>(query: &str, candidates: impl IntoIterator<Item = &'a str>) -> Vec<StringMatch> {
    let mut matches: Vec<StringMatch> = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(candidate_index, candidate)| {
            let (score, positions) = match_candidate(query, candidate)?;
            Some(StringMatch {
                candidate_index,
                score,
                positions,
            })
        })
        .collect();
    // A stable sort: equal scores keep the original order, which is what an empty query
    // produces for every candidate.
    matches.sort_by_key(|string_match| std::cmp::Reverse(string_match.score));
    matches
}

/// The strongest match and its grade: every alignment of `query` in `candidate` — every
/// way to spell the query out as a case-insensitive subsequence — is scored and the best
/// one wins, so a tight match later in the candidate outranks a loose earlier one.
///
/// The search carries partial alignments one query character at a time and merges all
/// that end at the same candidate position with the same tier facts, keeping it
/// `O(query × candidate²)` and free of recursion.
fn match_candidate(query: &str, candidate: &str) -> Option<(Score, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }

    let query_characters: Vec<char> = query.chars().collect();
    let candidate_characters: Vec<char> = candidate.chars().collect();
    let mut frontier = empty_frontier(candidate_characters.len());
    frontier[frontier_index(None, false, true, true)] = Some(PartialAlignment {
        previous_position: None,
        word_start_any: false,
        all_contiguous: true,
        exact_prefix: true,
        bonus: 0,
        positions: Vec::new(),
    });

    for (query_index, &query_character) in query_characters.iter().enumerate() {
        let mut next_frontier = empty_frontier(candidate_characters.len());
        for partial in frontier.iter().flatten() {
            let first_position = partial.previous_position.map_or(0, |position| position + 1);
            for (position, &candidate_character) in
                candidate_characters.iter().enumerate().skip(first_position)
            {
                if !characters_match(query_character, candidate_character) {
                    continue;
                }
                let previous_character = position
                    .checked_sub(1)
                    .and_then(|index| candidate_characters.get(index))
                    .copied();
                let word_start = is_word_start(previous_character, candidate_character);
                let contiguous = partial
                    .previous_position
                    .is_some_and(|last| last + 1 == position);
                let mut positions = partial.positions.clone();
                positions.push(position);
                keep_if_stronger(
                    &mut next_frontier,
                    PartialAlignment {
                        previous_position: Some(position),
                        word_start_any: partial.word_start_any || word_start,
                        all_contiguous: partial.all_contiguous
                            && (partial.previous_position.is_none() || contiguous),
                        exact_prefix: partial.exact_prefix && position == query_index,
                        bonus: partial
                            .bonus
                            .saturating_add(if word_start { WORD_START_BONUS } else { 0 })
                            .saturating_add(if contiguous { CONTIGUITY_BONUS } else { 0 }),
                        positions,
                    },
                );
            }
        }
        frontier = next_frontier;
        if frontier.iter().all(Option::is_none) {
            return None;
        }
    }

    strongest_alignment(&frontier, query_characters.len())
}

/// One partial alignment the search carries: where the last match landed, the facts the
/// tier is graded on (any word start, one contiguous run, an exact prefix so far), the
/// bonus earned so far, and the matched positions for highlighting.
#[derive(Clone)]
struct PartialAlignment {
    previous_position: Option<usize>,
    word_start_any: bool,
    all_contiguous: bool,
    exact_prefix: bool,
    bonus: Score,
    positions: Vec<usize>,
}

/// Frontier slot layout: one slot per (last matched position, tier flags) pair — the
/// three flags packed into the low bits of the index.
const FLAG_WORD_START_ANY: usize = 1;
const FLAG_ALL_CONTIGUOUS: usize = 2;
const FLAG_EXACT_PREFIX: usize = 4;
const FLAG_COUNT: usize = 8;

/// The frontier slot a partial alignment belongs to. Everything the remaining query can
/// still earn or land in depends only on the slot, so the strongest alignment per slot
/// is the only one worth keeping.
fn frontier_index(
    previous_position: Option<usize>,
    word_start_any: bool,
    all_contiguous: bool,
    exact_prefix: bool,
) -> usize {
    let previous = previous_position.map_or(0, |position| position + 1);
    (previous * FLAG_COUNT)
        | (usize::from(word_start_any) * FLAG_WORD_START_ANY)
        | (usize::from(all_contiguous) * FLAG_ALL_CONTIGUOUS)
        | (usize::from(exact_prefix) * FLAG_EXACT_PREFIX)
}

fn empty_frontier(candidate_length: usize) -> Vec<Option<PartialAlignment>> {
    vec![None; (candidate_length + 1) * FLAG_COUNT]
}

/// Keep `alignment` in its frontier slot when its bonus beats what is already there —
/// the slot fixes the tier the alignment can land in, so the bonus is all that separates
/// them. Ties keep the alignment found first, which is the leftmost one.
fn keep_if_stronger(frontier: &mut [Option<PartialAlignment>], alignment: PartialAlignment) {
    let index = frontier_index(
        alignment.previous_position,
        alignment.word_start_any,
        alignment.all_contiguous,
        alignment.exact_prefix,
    );
    let stronger = frontier[index]
        .as_ref()
        .is_none_or(|kept| alignment.bonus > kept.bonus);
    if stronger {
        frontier[index] = Some(alignment);
    }
}

/// The strongest finished alignment: the biggest tier it can land in (exact prefix,
/// then any word start, then one contiguous run) plus that alignment's bonus and one
/// point per matched character. Ties keep the first slot, so the result is deterministic.
fn strongest_alignment(
    frontier: &[Option<PartialAlignment>],
    query_length: usize,
) -> Option<(Score, Vec<usize>)> {
    let character_bonus = CHAR_BONUS.saturating_mul(counted(query_length));
    let mut best: Option<(Score, Vec<usize>)> = None;
    for partial in frontier.iter().flatten() {
        let tier = if partial.exact_prefix {
            TIER_EXACT_PREFIX
        } else if partial.word_start_any {
            TIER_WORD_START
        } else if partial.all_contiguous {
            TIER_CONTIGUOUS
        } else {
            0
        };
        let score = tier
            .saturating_add(partial.bonus)
            .saturating_add(character_bonus);
        let stronger = best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score);
        if stronger {
            best = Some((score, partial.positions.clone()));
        }
    }
    best
}

/// Case-insensitive per character: `'A'` matches `'a'`, in any script.
fn characters_match(query_character: char, candidate_character: char) -> bool {
    query_character
        .to_lowercase()
        .eq(candidate_character.to_lowercase())
}

/// Whether `character` starts a word: it follows one of [`WORD_START_AFTER`] or is a
/// camelCase hump. The start of the candidate is deliberately *not* a word start — the
/// exact-prefix tier already rewards it.
fn is_word_start(previous_character: Option<char>, character: char) -> bool {
    let Some(previous_character) = previous_character else {
        return false;
    };
    WORD_START_AFTER.contains(&previous_character)
        || (previous_character.is_lowercase() && character.is_uppercase())
}

/// A count as a [`Score`], clamped instead of wrapped: scores only ever add up.
fn counted(items: usize) -> Score {
    Score::try_from(items).unwrap_or(Score::MAX)
}

#[cfg(test)]
#[path = "fuzzy_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "fuzzy_alignment_tests.rs"]
mod alignment_tests;
