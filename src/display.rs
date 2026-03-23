use crate::AppResult;
use chrono::{DateTime, Local, Utc};
use similar::TextDiff;
use std::path::{Path, PathBuf};

pub fn current_project_path() -> AppResult<PathBuf> {
    let cwd = std::env::current_dir()?;
    Ok(cwd.canonicalize().unwrap_or(cwd))
}

pub fn token_count(text: &str) -> usize {
    text.split_whitespace().count()
}

pub fn count_rules(text: &str) -> usize {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("## ")
                || trimmed
                    .chars()
                    .next()
                    .map(|ch| ch.is_ascii_digit())
                    .unwrap_or(false)
        })
        .count()
}

pub fn percentage(numerator: usize, denominator: usize) -> usize {
    if denominator == 0 {
        0
    } else {
        ((numerator as f64 / denominator as f64) * 100.0).round() as usize
    }
}

pub fn ratio_phrase(rate: f64) -> String {
    if rate <= 0.0 {
        return "almost never".to_string();
    }
    if rate >= 1.0 {
        return "every session".to_string();
    }
    let reciprocal = (1.0 / rate).round() as usize;
    if reciprocal <= 1 {
        "about every session".to_string()
    } else {
        format!("about 1 in {reciprocal}")
    }
}

pub fn approximate_cost(input_tokens: i64, output_tokens: i64) -> f64 {
    let input_million = input_tokens.max(0) as f64 / 1_000_000.0;
    let output_million = output_tokens.max(0) as f64 / 1_000_000.0;
    input_million * 3.0 + output_million * 15.0
}

pub fn format_box(text: &str) -> String {
    text.lines()
        .map(|line| format!("  │  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_unified_diff(old_text: &str, new_text: &str) -> String {
    TextDiff::from_lines(old_text, new_text)
        .unified_diff()
        .context_radius(3)
        .header("AGENTS.md", "AGENTS.md (proposed)")
        .to_string()
}

pub fn human_period(min_started_at: Option<&str>, max_started_at: Option<&str>) -> String {
    match (min_started_at, max_started_at) {
        (Some(min), Some(max)) => format!(
            "{} to {}",
            human_date(min).unwrap_or_else(|| min.to_string()),
            human_date(max).unwrap_or_else(|| max.to_string())
        ),
        _ => "the last 30 days".to_string(),
    }
}

pub fn human_date(timestamp: &str) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(timestamp).ok()?;
    Some(parsed.with_timezone(&Local).format("%Y-%m-%d").to_string())
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

pub fn normalize_file_path(file_path: &str, project_path: Option<&Path>) -> String {
    let candidate = PathBuf::from(file_path);
    if candidate.is_absolute() {
        if let Some(project_path) = project_path {
            if let Ok(stripped) = candidate.strip_prefix(project_path) {
                return stripped.to_string_lossy().to_string();
            }
        }
    }
    file_path.replace('\\', "/")
}

pub fn top_codebase_area(file_path: &str) -> Option<String> {
    let segments = file_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    if segments.is_empty() {
        None
    } else if segments.len() == 1 {
        let stem = Path::new(segments[0])
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(segments[0]);
        Some(format!("{stem}/"))
    } else if segments.len() == 2 && segments[1].contains('.') {
        let stem = Path::new(segments[1])
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(segments[1]);
        Some(format!("{}/{stem}/", segments[0]))
    } else {
        Some(format!("{}/{}/", segments[0], segments[1]))
    }
}

pub fn mask_secret(secret: &str) -> String {
    if secret.len() <= 8 {
        return "********".to_string();
    }
    format!("{}…{}", &secret[..4], &secret[secret.len() - 4..])
}
