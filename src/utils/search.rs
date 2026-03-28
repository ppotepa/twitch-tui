use std::sync::LazyLock;

use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};

pub static FUZZY_FINDER: LazyLock<SkimMatcherV2> = LazyLock::new(SkimMatcherV2::default);

pub fn strict_pattern_match(pattern: &str, choice: &str) -> Vec<usize> {
    let normalized_pattern = pattern
        .trim()
        .chars()
        .filter(|c| *c != '*')
        .collect::<String>()
        .to_lowercase();

    if normalized_pattern.is_empty() {
        return vec![];
    }

    let choice_lower = choice.to_lowercase();
    if let Some(start) = choice_lower.find(&normalized_pattern) {
        return (start..start + normalized_pattern.len()).collect();
    }

    vec![]
}

pub fn fuzzy_pattern_match(pattern: &str, choice: &str) -> Vec<usize> {
    FUZZY_FINDER
        .fuzzy_indices(choice, pattern)
        .map(|(_, indices)| indices)
        .unwrap_or_default()
}
