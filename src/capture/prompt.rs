use crate::analysis::prompt_features;
use crate::config::HarnConfig;
use crate::db::{self, queries};
use crate::display;
use crate::AppResult;
use serde::Deserialize;
use std::io::Read;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Deserialize)]
struct PromptInput {
    session_id: String,
    prompt: String,
    cwd: Option<String>,
    transcript_path: Option<String>,
}

pub fn handle_from_stdin() -> AppResult<()> {
    let input = read_input()?;
    let config = HarnConfig::load();
    let project_path = input
        .cwd
        .as_deref()
        .map(normalize_project_path)
        .transpose()?;

    if let Some(project_path) = project_path.as_deref() {
        if config.is_excluded_project(project_path) {
            return Ok(());
        }
    }

    let git_branch = project_path
        .as_deref()
        .and_then(|path| git_branch(Path::new(path)).ok());
    let conn = db::connect()?;
    let timestamp = display::now_rfc3339();

    let _ = input.transcript_path;
    queries::upsert_prompt_session(
        &conn,
        &input.session_id,
        project_path.as_deref(),
        git_branch.as_deref(),
        &input.prompt,
        &timestamp,
        "hook",
    )?;

    // Classify and store the prompt turn
    let existing_turns = queries::session_turn_count(&conn, &input.session_id).unwrap_or(0);
    let turn_number = existing_turns + 1;
    let word_count = input.prompt.split_whitespace().count() as i64;
    let classification = prompt_features::classify_turn(turn_number as usize, &input.prompt);
    let _ = queries::insert_prompt_turn(
        &conn,
        &input.session_id,
        turn_number,
        &timestamp,
        &input.prompt,
        word_count,
        classification,
    );

    Ok(())
}

fn read_input() -> AppResult<PromptInput> {
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    Ok(serde_json::from_str(&buffer)?)
}

fn normalize_project_path(path: &str) -> AppResult<String> {
    let candidate = Path::new(path);
    Ok(candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.to_path_buf())
        .to_string_lossy()
        .to_string())
}

fn git_branch(project_path: &Path) -> AppResult<String> {
    let output = Command::new("git")
        .current_dir(project_path)
        .args(["branch", "--show-current"])
        .output()?;

    if !output.status.success() {
        return Err(crate::boxed_error("unable to determine git branch"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
