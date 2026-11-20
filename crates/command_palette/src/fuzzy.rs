//! The fuzzy scorer the [`crate::Picker`] ranks its matches with.
//!
//! A simple scorer on purpose: `score` walks the query through the candidate as a
//! case-insensitive subsequence and grades where the match landed — an exact prefix
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

/// The greedy match and its grade: the leftmost subsequence of `candidate` the query
/// spells out, graded by where it landed. The leftmost match is a deliberate simplicity
/// — a query box's candidates are short, and a smarter alignment is a future improvement.
fn match_candidate(query: &str, candidate: &str) -> Option<(Score, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }

    let query_length = query.chars().count();
    let mut positions = Vec::new();
    let mut word_start_hits: usize = 0;
    let mut contiguous_pairs: usize = 0;
    let mut previous_character: Option<char> = None;
    let mut query_characters = query.chars().peekable();

    // `char_indices` keeps us honest about multi-byte characters while `enumerate`
    // numbers positions in characters — positions are char indices, never byte offsets.
    for (position, (_byte_offset, character)) in candidate.char_indices().enumerate() {
        let Some(&query_character) = query_characters.peek() else {
            break;
        };
        if !characters_match(query_character, character) {
            previous_character = Some(character);
            continue;
        }
        if positions.last().is_some_and(|&last| last + 1 == position) {
            contiguous_pairs += 1;
        }
        if is_word_start(previous_character, character) {
            word_start_hits += 1;
        }
        positions.push(position);
        query_characters.next();
        previous_character = Some(character);
    }

    if query_characters.next().is_some() {
        return None;
    }

    let exact_prefix = positions.last() == Some(&(query_length - 1));
    let tier = if exact_prefix {
        TIER_EXACT_PREFIX
    } else if word_start_hits > 0 {
        TIER_WORD_START
    } else if contiguous_pairs + 1 == positions.len() {
        TIER_CONTIGUOUS
    } else {
        0
    };
    let bonuses = WORD_START_BONUS
        .saturating_mul(counted(word_start_hits))
        .saturating_add(CONTIGUITY_BONUS.saturating_mul(counted(contiguous_pairs)))
        .saturating_add(CHAR_BONUS.saturating_mul(counted(positions.len())));
    Some((tier.saturating_add(bonuses), positions))
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
