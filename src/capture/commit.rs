use crate::db::{self, queries};
use crate::diff;
use crate::display;
use crate::AppResult;
use chrono::{Duration, Utc};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn handle(
    commit_hash: &str,
    branch: Option<&str>,
    project_path: Option<&str>,
) -> AppResult<()> {
    let conn = db::connect()?;
    let project_path = resolve_project_path(project_path)?;
    let project_string = project_path.to_string_lossy().to_string();
    let files_changed = git_files_changed(&project_path, commit_hash)?;
    let (insertions, deletions) = git_shortstat(&project_path, commit_hash)?;
    let timestamp =
        git_commit_timestamp(&project_path, commit_hash).unwrap_or_else(|_| display::now_rfc3339());
    let branch = branch
        .map(ToOwned::to_owned)
        .or_else(|| current_branch(&project_path).ok());

    let session_id = match_session(&conn, &project_string, &files_changed)?;
    queries::insert_commit(
        &conn,
        commit_hash,
        session_id.as_deref(),
        branch.as_deref(),
        &timestamp,
        &serde_json::to_string(&files_changed)?,
        insertions,
        deletions,
    )?;

    if let Some(session_id) = session_id.as_deref() {
        let touched_files = queries::session_file_paths(&conn, session_id)?;
        let changed_set = files_changed.iter().cloned().collect::<HashSet<_>>();
        for file_path in touched_files
            .into_iter()
            .filter(|path| changed_set.contains(path))
        {
            if let Some(edit) =
                diff::compute_edit_for_commit(&project_path, commit_hash, &file_path)?
            {
                queries::insert_edit(
                    &conn,
                    commit_hash,
                    session_id,
                    &edit.file_path,
                    edit.ai_lines,
                    edit.human_kept,
                    edit.human_modified,
                    edit.human_deleted,
                    edit.human_added,
                    edit.acceptance_rate,
                )?;
            }
        }
    }

    Ok(())
}

fn resolve_project_path(project_path: Option<&str>) -> AppResult<PathBuf> {
    let path = project_path
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    Ok(path.canonicalize().unwrap_or(path))
}

fn git_files_changed(project_path: &Path, commit_hash: &str) -> AppResult<Vec<String>> {
    let output = Command::new("git")
        .current_dir(project_path)
        .args([
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            commit_hash,
        ])
        .output()?;
    if !output.status.success() {
        return Err(crate::boxed_error("unable to read changed files from git"));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn git_shortstat(project_path: &Path, commit_hash: &str) -> AppResult<(i64, i64)> {
    let output = Command::new("git")
        .current_dir(project_path)
        .args([
            "diff",
            "--shortstat",
            &format!("{commit_hash}~1"),
            commit_hash,
        ])
        .output()?;
    if !output.status.success() {
        return Ok((0, 0));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut numbers = text
        .split_whitespace()
        .filter_map(|part| part.parse::<i64>().ok());
    let _files_changed = numbers.next().unwrap_or(0);
    let insertions = numbers.next().unwrap_or(0);
    let deletions = numbers.next().unwrap_or(0);
    Ok((insertions, deletions))
}

fn git_commit_timestamp(project_path: &Path, commit_hash: &str) -> AppResult<String> {
    let output = Command::new("git")
        .current_dir(project_path)
        .args(["show", "-s", "--format=%cI", commit_hash])
        .output()?;
    if !output.status.success() {
        return Err(crate::boxed_error("unable to read commit timestamp"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn current_branch(project_path: &Path) -> AppResult<String> {
    let output = Command::new("git")
        .current_dir(project_path)
        .args(["branch", "--show-current"])
        .output()?;
    if !output.status.success() {
        return Err(crate::boxed_error("unable to read branch"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn match_session(
    conn: &rusqlite::Connection,
    project_path: &str,
    files_changed: &[String],
) -> AppResult<Option<String>> {
    let since = (Utc::now() - Duration::minutes(30)).to_rfc3339();
    let candidates = queries::recent_candidate_sessions(conn, project_path, &since)?;
    if candidates.is_empty() {
        return Ok(None);
    }

    let changed = files_changed.iter().cloned().collect::<HashSet<_>>();
    let mut best_match: Option<(usize, String, String)> = None;

    for candidate in candidates {
        let file_paths = queries::session_file_paths(conn, &candidate.session_id)?;
        let overlap = file_paths
            .iter()
            .filter(|file_path| changed.contains(*file_path))
            .count();
        match &best_match {
            Some((best_overlap, _, best_started_at)) => {
                if overlap > *best_overlap
                    || (overlap == *best_overlap && candidate.started_at > *best_started_at)
                {
                    best_match = Some((overlap, candidate.session_id, candidate.started_at));
                }
            }
            None => {
                best_match = Some((overlap, candidate.session_id, candidate.started_at));
            }
        }
    }

    if let Some((overlap, session_id, _)) = best_match {
        if overlap > 0 || files_changed.is_empty() {
            return Ok(Some(session_id));
        }
        if candidates_len_is_one(conn, project_path, &since)? {
            return Ok(Some(session_id));
        }
    }

    Ok(None)
}

fn candidates_len_is_one(
    conn: &rusqlite::Connection,
    project_path: &str,
    since: &str,
) -> AppResult<bool> {
    let count: i64 = conn.query_row(
        "
        SELECT COUNT(*)
        FROM sessions
        WHERE project_path = ?1
          AND started_at >= ?2
          AND outcome = 'in_progress'
        ",
        rusqlite::params![project_path, since],
        |row| row.get(0),
    )?;
    Ok(count == 1)
}
