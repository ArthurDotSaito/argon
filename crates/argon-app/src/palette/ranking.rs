use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

const EXACT_NUMBER_BONUS: u32 = 2_000_000;
const PREFIX_NUMBER_BONUS: u32 = 1_000_000;
const MAX_MATCH_SPREAD: usize = 2;

#[derive(Clone, Copy)]
pub struct Candidate<'a> {
    pub text: &'a str,
    pub number: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scored {
    pub index: usize,
    pub score: u32,
}

pub fn pr_number(query: &str) -> Option<&str> {
    let digits = query.trim().trim_start_matches('#');
    (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())).then_some(digits)
}

pub fn prs_first(query: &str, prs: &[Scored], commands: &[Scored]) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return false;
    }
    if pr_number(query).is_some() {
        return true;
    }
    let top = |ranked: &[Scored]| ranked.first().map_or(0, |scored| scored.score);
    top(prs) > top(commands)
}

pub struct Ranker {
    matcher: Matcher,
    buffer: Vec<char>,
    indices: Vec<u32>,
}

impl Ranker {
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            buffer: Vec::new(),
            indices: Vec::new(),
        }
    }

    pub fn rank<'a>(
        &mut self,
        query: &str,
        candidates: impl IntoIterator<Item = Candidate<'a>>,
    ) -> Vec<Scored> {
        let query = query.trim();
        let number_query = pr_number(query);
        let pattern = Pattern::new(
            number_query.unwrap_or(query),
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut ranked: Vec<Scored> = candidates
            .into_iter()
            .enumerate()
            .filter_map(|(index, candidate)| {
                let score = match number_query {
                    Some(digits) => self.score_number(&pattern, digits, candidate),
                    None => self.fuzzy(&pattern, candidate.text),
                }?;
                Some(Scored { index, score })
            })
            .collect();
        ranked.sort_by_key(|scored| std::cmp::Reverse(scored.score));
        ranked
    }

    fn fuzzy(&mut self, pattern: &Pattern, text: &str) -> Option<u32> {
        let haystack = Utf32Str::new(text, &mut self.buffer);
        let mut total = 0;
        for atom in &pattern.atoms {
            self.indices.clear();
            let score = atom.indices(haystack, &mut self.matcher, &mut self.indices)?;
            let first = self.indices.iter().min().copied().unwrap_or_default();
            let last = self.indices.iter().max().copied().unwrap_or_default();
            let span = (last - first) as usize + 1;
            if span > atom.needle_text().len() * MAX_MATCH_SPREAD {
                return None;
            }
            total += u32::from(score);
        }
        Some(total)
    }

    fn score_number(
        &mut self,
        pattern: &Pattern,
        digits: &str,
        candidate: Candidate<'_>,
    ) -> Option<u32> {
        let bonus = number_bonus(digits, candidate.number);
        let text_score = if candidate.text.contains(digits) {
            self.fuzzy(pattern, candidate.text)
        } else {
            None
        };
        match (bonus, text_score) {
            (0, None) => None,
            (bonus, text_score) => Some(bonus + text_score.unwrap_or(0)),
        }
    }
}

fn number_bonus(digits: &str, number: Option<u64>) -> u32 {
    let Some(number) = number else {
        return 0;
    };
    if digits.parse::<u64>().ok() == Some(number) {
        EXACT_NUMBER_BONUS
    } else if number.to_string().starts_with(digits) {
        PREFIX_NUMBER_BONUS
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(text: &str, number: Option<u64>) -> Candidate<'_> {
        Candidate { text, number }
    }

    fn order(ranked: &[Scored]) -> Vec<usize> {
        ranked.iter().map(|scored| scored.index).collect()
    }

    #[test]
    fn pr_number_accepts_digits_with_optional_hash() {
        assert_eq!(pr_number("482"), Some("482"));
        assert_eq!(pr_number("#482"), Some("482"));
        assert_eq!(pr_number("  #482 "), Some("482"));
        assert_eq!(pr_number("48a"), None);
        assert_eq!(pr_number("#"), None);
        assert_eq!(pr_number(""), None);
    }

    #[test]
    fn number_query_puts_exact_then_prefix_number_first() {
        let mut ranker = Ranker::new();
        let ranked = ranker.rank(
            "482",
            [
                candidate("Fix slot 482 overflow\nacme/api #17", Some(17)),
                candidate("Bump deps\nacme/api #4821", Some(4821)),
                candidate("Add retries\nacme/api #482", Some(482)),
                candidate("Unrelated\nacme/web #9", Some(9)),
            ],
        );
        assert_eq!(order(&ranked), vec![2, 1, 0]);
    }

    #[test]
    fn hash_prefix_ranks_like_plain_digits() {
        let mut ranker = Ranker::new();
        let ranked = ranker.rank(
            "#482",
            [
                candidate("Other\nacme/api #4820", Some(4820)),
                candidate("Target\nacme/api #482", Some(482)),
            ],
        );
        assert_eq!(order(&ranked), vec![1, 0]);
    }

    #[test]
    fn empty_query_keeps_original_order() {
        let mut ranker = Ranker::new();
        let ranked = ranker.rank(
            "  ",
            [
                candidate("b", None),
                candidate("a", None),
                candidate("c", None),
            ],
        );
        assert_eq!(order(&ranked), vec![0, 1, 2]);
    }

    #[test]
    fn fuzzy_query_drops_misses_and_widely_scattered_matches() {
        let mut ranker = Ranker::new();
        let ranked = ranker.rank(
            "needs",
            [
                candidate("nothing to see here", None),
                candidate("never end every day, so sleep", None),
                candidate("Ir para Needs you", None),
            ],
        );
        assert_eq!(order(&ranked), vec![2]);
    }

    #[test]
    fn fuzzy_query_accepts_compact_gaps_and_prefers_tighter_matches() {
        let mut ranker = Ranker::new();
        let ranked = ranker.rank(
            "sync",
            [
                candidate("s-y-n-c thing", None),
                candidate("sync worker", None),
            ],
        );
        assert_eq!(order(&ranked), vec![1, 0]);
    }

    #[test]
    fn match_spread_is_measured_in_chars_not_bytes() {
        let mut ranker = Ranker::new();
        let ranked = ranker.rank(
            "ção",
            [
                candidate("ç.ã.o", None),
                candidate("ç....ã..o", None),
                candidate("açã", None),
            ],
        );
        assert_eq!(order(&ranked), vec![0]);
    }

    #[test]
    fn prs_first_follows_query_shape_and_top_score() {
        let high = [Scored {
            index: 0,
            score: 90,
        }];
        let low = [Scored {
            index: 0,
            score: 40,
        }];
        assert!(!prs_first("", &high, &low));
        assert!(prs_first("482", &low, &high));
        assert!(prs_first("#482", &low, &high));
        assert!(prs_first("fix", &high, &low));
        assert!(!prs_first("fix", &low, &high));
        assert!(!prs_first("fix", &[], &[]));
    }
}
