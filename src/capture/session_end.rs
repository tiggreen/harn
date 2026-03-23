use crate::capture::thrashing;
use crate::db::{self, queries};
use crate::display;
use crate::AppResult;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct SessionEndInput {
    session_id: String,
    reason: Option<String>,
    cwd: Option<String>,
    transcript_path: Option<String>,
}

pub fn handle_from_stdin() -> AppResult<()> {
    let input = read_input()?;
    let conn = db::connect()?;
    let now = display::now_rfc3339();
    let cwd = input
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);

    // Detect thrashing before classifying outcome (classifier uses thrashing data)
    let _ = thrashing::detect_and_store(&conn, &input.session_id);

    let has_commit = queries::session_has_commit(&conn, &input.session_id)?;
    let stats = queries::session_tool_stats(&conn, &input.session_id)?;
    let outcome = queries::classify_outcome(has_commit, &stats);
    queries::finalize_session(
        &conn,
        &input.session_id,
        Some(&cwd.to_string_lossy()),
        &now,
        outcome,
        input.reason.as_deref(),
    )?;
    snapshot_harness_files(&conn, &cwd, Some(&input.session_id))?;
    let _ = input.transcript_path;
    Ok(())
}

pub fn snapshot_harness_files(
    conn: &rusqlite::Connection,
    cwd: &Path,
    session_id: Option<&str>,
) -> AppResult<usize> {
    let mut files = Vec::new();
    files.push(cwd.join("AGENTS.md"));
    files.push(cwd.join("CLAUDE.md"));

    let commands_dir = cwd.join(".claude").join("commands");
    if commands_dir.exists() {
        for entry in fs::read_dir(commands_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("md") {
                files.push(path);
            }
        }
    }

    let captured_at = display::now_rfc3339();
    let mut snapshot_count = 0;
    for path in files {
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let content_hash = format!("{:x}", hasher.finalize());
        let relative = path
            .strip_prefix(cwd)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();

        queries::insert_harness_snapshot(
            conn,
            session_id,
            &relative,
            &content_hash,
            &content,
            display::token_count(&content) as i64,
            &captured_at,
        )?;
        snapshot_count += 1;
    }

    Ok(snapshot_count)
}

fn read_input() -> AppResult<SessionEndInput> {
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    Ok(serde_json::from_str(&buffer)?)
}
