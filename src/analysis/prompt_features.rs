use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptFeatures {
    pub word_count: usize,
    pub has_file_path: bool,
    pub has_reference_impl: bool,
    pub has_negative_constraint: bool,
    pub specific: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptAggregate {
    pub avg_prompt_length_words: f64,
    pub pct_with_file_path: f64,
    pub pct_with_reference_impl: f64,
    pub pct_with_negative_constraint: f64,
    pub pct_specific: f64,
}

pub fn analyze(prompt: &str) -> PromptFeatures {
    let lower = prompt.to_lowercase();
    let word_count = prompt.split_whitespace().count();
    let has_file_path = prompt.contains('/') || lower.contains("src/") || lower.contains(".rs");
    let has_reference_impl = ["like ", "similar to", "see ", "follow ", "based on"]
        .iter()
        .any(|needle| lower.contains(needle));
    let has_negative_constraint = ["do not", "don't", "never", "without", "avoid "]
        .iter()
        .any(|needle| lower.contains(needle));
    let specific =
        word_count >= 12 || has_file_path || has_reference_impl || has_negative_constraint;

    PromptFeatures {
        word_count,
        has_file_path,
        has_reference_impl,
        has_negative_constraint,
        specific,
    }
}

const CORRECTION_KEYWORDS: &[&str] = &[
    "no,", "no ", "not that", "wrong", "instead", "actually,", "actually ", "wait", "stop",
    "undo", "revert", "try again", "that's not", "don't", "shouldn't", "that is not",
    "do not", "i meant", "i said", "use the other", "the other one", "go back",
];

pub fn classify_turn(turn_number: usize, prompt: &str) -> &'static str {
    if turn_number <= 1 {
        return "initial_prompt";
    }

    let lower = prompt.to_lowercase();
    let word_count = prompt.split_whitespace().count();

    // Very short messages after the first turn are almost always corrections or acknowledgments
    if word_count <= 6 {
        return "correction";
    }

    // Short messages with redirective language
    if word_count <= 20 && CORRECTION_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return "correction";
    }

    // Messages that start with "no" or "stop"
    let trimmed = lower.trim();
    if trimmed.starts_with("no ") || trimmed.starts_with("no,") || trimmed.starts_with("stop") {
        return "correction";
    }

    "follow_up"
}

pub fn aggregate<'a>(prompts: impl Iterator<Item = &'a str>) -> PromptAggregate {
    let mut count = 0_usize;
    let mut total_words = 0_usize;
    let mut with_file_path = 0_usize;
    let mut with_reference_impl = 0_usize;
    let mut with_negative_constraint = 0_usize;
    let mut specific_count = 0_usize;

    for prompt in prompts {
        let features = analyze(prompt);
        count += 1;
        total_words += features.word_count;
        with_file_path += usize::from(features.has_file_path);
        with_reference_impl += usize::from(features.has_reference_impl);
        with_negative_constraint += usize::from(features.has_negative_constraint);
        specific_count += usize::from(features.specific);
    }

    if count == 0 {
        return PromptAggregate::default();
    }

    PromptAggregate {
        avg_prompt_length_words: total_words as f64 / count as f64,
        pct_with_file_path: with_file_path as f64 / count as f64,
        pct_with_reference_impl: with_reference_impl as f64 / count as f64,
        pct_with_negative_constraint: with_negative_constraint as f64 / count as f64,
        pct_specific: specific_count as f64 / count as f64,
    }
}
