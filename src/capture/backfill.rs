use crate::config::HarnConfig;
use crate::db::queries;
use crate::display;
use crate::AppResult;
use chrono::{Duration, Utc};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct BackfillStats {
    pub imported_sessions: usize,
    pub imported_tool_calls: usize,
    pub skipped_sessions: usize,
    pub imported_current_project_sessions: usize,
    pub imported_other_project_sessions: usize,
    pub skipped_current_project_sessions: usize,
    pub skipped_other_project_sessions: usize,
}

#[derive(Debug)]
struct ParsedSession {
    session_id: String,
    project_path: Option<String>,
    git_branch: Option<String>,
    started_at: String,
    ended_at: Option<String>,
    prompt_text: Option<String>,
    total_turns: i64,
    total_tokens_in: i64,
    total_tokens_out: i64,
    outcome: String,
    end_reason: Option<String>,
    tool_calls: Vec<ParsedToolCall>,
}

#[derive(Debug, Clone)]
struct ParsedToolCall {
    timestamp: String,
    tool_name: String,
    file_path: Option<String>,
    command: Option<String>,
    success: bool,
    error_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TranscriptLine {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "gitBranch")]
    git_branch: Option<String>,
    timestamp: Option<String>,
    message: Option<TranscriptMessage>,
}

#[derive(Debug, Deserialize)]
struct TranscriptMessage {
    role: Option<String>,
    content: Value,
    usage: Option<MessageUsage>,
}

#[derive(Debug, Deserialize)]
struct MessageUsage {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
}

pub fn run_backfill(
    conn: &Connection,
    days: i64,
    config: &HarnConfig,
    current_project_path: Option<&str>,
) -> AppResult<BackfillStats> {
    let Some(root) = claude_projects_root() else {
        return Ok(BackfillStats::default());
    };

    let mut files = Vec::new();
    collect_jsonl_files(&root, &mut files)?;

    let since = (Utc::now() - Duration::days(days)).to_rfc3339();
    let mut stats = BackfillStats::default();

    for file in files {
        if let Some(parsed) = parse_session_file(&file, config.idle_timeout)? {
            let is_current_project = parsed.project_path.as_deref() == current_project_path;
            if parsed.started_at < since {
                continue;
            }
            if let Some(project_path) = parsed.project_path.as_deref() {
                if config.is_excluded_project(project_path) {
                    continue;
                }
            }
            if queries::session_exists(conn, &parsed.session_id)? {
                stats.skipped_sessions += 1;
                if is_current_project {
                    stats.skipped_current_project_sessions += 1;
                } else {
                    stats.skipped_other_project_sessions += 1;
                }
                continue;
            }

            queries::update_session_metadata(
                conn,
                &parsed.session_id,
                parsed.project_path.as_deref(),
                parsed.git_branch.as_deref(),
                Some(&parsed.started_at),
                parsed.ended_at.as_deref(),
                parsed.prompt_text.as_deref(),
                parsed
                    .prompt_text
                    .as_deref()
                    .map(display::token_count)
                    .unwrap_or(0) as i64,
                parsed.total_turns,
                parsed.total_tokens_in,
                parsed.total_tokens_out,
                &parsed.outcome,
                parsed.end_reason.as_deref(),
                "backfill",
            )?;

            for tool_call in parsed.tool_calls {
                queries::insert_tool_call(
                    conn,
                    &parsed.session_id,
                    &tool_call.timestamp,
                    &tool_call.tool_name,
                    tool_call.file_path.as_deref(),
                    tool_call.command.as_deref(),
                    tool_call.success,
                    tool_call.error_text.as_deref(),
                )?;
                stats.imported_tool_calls += 1;
            }

            stats.imported_sessions += 1;
            if is_current_project {
                stats.imported_current_project_sessions += 1;
            } else {
                stats.imported_other_project_sessions += 1;
            }
        }
    }

    Ok(stats)
}

fn claude_projects_root() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".claude").join("projects"))
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) -> AppResult<()> {
    if !root.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }

    Ok(())
}

fn parse_session_file(path: &Path, idle_timeout: u64) -> AppResult<Option<ParsedSession>> {
    let contents = fs::read_to_string(path)?;
    if contents.trim().is_empty() {
        return Ok(None);
    }

    let mut session_id = None;
    let mut project_path = None;
    let mut git_branch = None;
    let mut started_at = None;
    let mut ended_at = None;
    let mut prompt_text = None;
    let mut total_turns = 0_i64;
    let mut total_tokens_in = 0_i64;
    let mut total_tokens_out = 0_i64;
    let mut tool_calls = Vec::new();
    let mut tool_index_by_id = HashMap::<String, usize>::new();

    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let event: TranscriptLine = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if session_id.is_none() {
            session_id = event.session_id.clone();
        }
        if project_path.is_none() {
            project_path = event.cwd.clone().map(|cwd| normalize_project_path(&cwd));
        }
        if git_branch.is_none() {
            git_branch = event.git_branch.clone();
        }
        if let Some(timestamp) = event.timestamp.clone() {
            if started_at
                .as_ref()
                .map(|current| timestamp < *current)
                .unwrap_or(true)
            {
                started_at = Some(timestamp.clone());
            }
            if ended_at
                .as_ref()
                .map(|current| timestamp > *current)
                .unwrap_or(true)
            {
                ended_at = Some(timestamp);
            }
        }

        if let Some(message) = event.message {
            if message.role.as_deref() == Some("user") {
                total_turns += 1;
                if prompt_text.is_none() {
                    prompt_text = extract_text_content(&message.content);
                }
            }

            if let Some(usage) = message.usage {
                total_tokens_in += usage.input_tokens.unwrap_or(0)
                    + usage.cache_creation_input_tokens.unwrap_or(0)
                    + usage.cache_read_input_tokens.unwrap_or(0);
                total_tokens_out += usage.output_tokens.unwrap_or(0);
            }

            if let Value::Array(blocks) = message.content {
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        let tool_name = block
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("Unknown")
                            .to_string();
                        let input = block.get("input").cloned().unwrap_or(Value::Null);
                        let file_path = extract_file_path(&input).map(|value| {
                            display::normalize_file_path(
                                &value,
                                project_path.as_deref().map(Path::new),
                            )
                        });
                        let parsed = ParsedToolCall {
                            timestamp: ended_at.clone().unwrap_or_else(display::now_rfc3339),
                            tool_name,
                            file_path,
                            command: extract_command(&input),
                            success: true,
                            error_text: None,
                        };
                        tool_calls.push(parsed);
                        if let Some(tool_use_id) = block.get("id").and_then(Value::as_str) {
                            tool_index_by_id.insert(tool_use_id.to_string(), tool_calls.len() - 1);
                        }
                    } else if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                        let Some(tool_use_id) = block.get("tool_use_id").and_then(Value::as_str)
                        else {
                            continue;
                        };
                        if let Some(index) = tool_index_by_id.get(tool_use_id).copied() {
                            let is_error = block
                                .get("is_error")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            tool_calls[index].success = !is_error;
                            if is_error {
                                tool_calls[index].error_text = extract_text_content(
                                    &block.get("content").cloned().unwrap_or(Value::Null),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    let Some(session_id) = session_id else {
        return Ok(None);
    };
    let Some(started_at) = started_at else {
        return Ok(None);
    };

    let ended_at_value = ended_at.clone();
    let idle_cutoff = Utc::now() - Duration::seconds(idle_timeout as i64);
    let is_ended = ended_at_value
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc) < idle_cutoff)
        .unwrap_or(false);

    let outcome = if !is_ended {
        "in_progress".to_string()
    } else {
        let has_edits = tool_calls.iter().any(|tc| {
            matches!(tc.tool_name.as_str(), "Edit" | "Write" | "MultiEdit")
        });
        let failed = tool_calls.iter().filter(|tc| !tc.success).count() as i64;
        let total = tool_calls.len() as i64;
        let failure_rate = if total > 0 { failed as f64 / total as f64 } else { 0.0 };

        if !has_edits {
            "exploratory".to_string()
        } else if failure_rate > 0.3 {
            "failed".to_string()
        } else {
            "abandoned".to_string()
        }
    };

    Ok(Some(ParsedSession {
        session_id,
        project_path,
        git_branch,
        started_at,
        ended_at: ended_at_value,
        prompt_text,
        total_turns,
        total_tokens_in,
        total_tokens_out,
        outcome,
        end_reason: None,
        tool_calls,
    }))
}

fn normalize_project_path(path: &str) -> String {
    let candidate = Path::new(path);
    candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn extract_text_content(content: &Value) -> Option<String> {
    match content {
        Value::String(value) => Some(value.clone()),
        Value::Array(values) => values
            .iter()
            .filter_map(|value| {
                if value.get("type").and_then(Value::as_str) == Some("text") {
                    value
                        .get("text")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                } else {
                    None
                }
            })
            .reduce(|mut acc, value| {
                if !acc.is_empty() {
                    acc.push('\n');
                }
                acc.push_str(&value);
                acc
            }),
        Value::Object(_) => content
            .get("text")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        _ => None,
    }
}

fn extract_file_path(input: &Value) -> Option<String> {
    input
        .get("file_path")
        .and_then(Value::as_str)
        .or_else(|| input.get("path").and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

fn extract_command(input: &Value) -> Option<String> {
    input
        .get("command")
        .and_then(Value::as_str)
        .or_else(|| input.get("cmd").and_then(Value::as_str))
        .map(ToOwned::to_owned)
}
