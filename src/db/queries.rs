use crate::display;
use crate::scope::AnalysisScope;
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub project_path: Option<String>,
    pub git_branch: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub prompt_text: Option<String>,
    pub prompt_word_count: i64,
    pub total_turns: i64,
    pub total_tokens_in: i64,
    pub total_tokens_out: i64,
    pub outcome: String,
    pub end_reason: Option<String>,
    pub captured_via: String,
}

#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub session_id: String,
    pub timestamp: String,
    pub tool_name: String,
    pub file_path: Option<String>,
    pub command: Option<String>,
    pub success: bool,
    pub error_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectSummary {
    pub total_sessions: usize,
    pub committed: usize,
    pub abandoned: usize,
    pub failed: usize,
    pub exploratory: usize,
    pub in_progress: usize,
    pub avg_turns: f64,
    pub total_tokens_in: i64,
    pub total_tokens_out: i64,
    pub min_started_at: Option<String>,
    pub max_started_at: Option<String>,
    pub acceptance_rate: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ScopeBreakdown {
    pub current_project_sessions: usize,
    pub other_project_sessions: usize,
    pub project_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisRunRecord {
    pub id: i64,
    pub run_at: String,
    pub sessions_analyzed: i64,
    pub harness_score: i64,
    pub findings_json: String,
    pub generated_configs_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThrashingRecord {
    pub session_id: String,
    pub thrash_type: String,
    pub target: String,
    pub cycle_count: i64,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTurnRecord {
    pub session_id: String,
    pub turn_number: i64,
    pub timestamp: String,
    pub prompt_text: String,
    pub word_count: i64,
    pub classification: String,
}

pub fn session_exists(conn: &Connection, session_id: &str) -> Result<bool> {
    let value: Option<String> = conn
        .query_row(
            "SELECT session_id FROM sessions WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value.is_some())
}

pub fn session_project_path(conn: &Connection, session_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT project_path FROM sessions WHERE session_id = ?1",
        [session_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn ensure_session(
    conn: &Connection,
    session_id: &str,
    project_path: Option<&str>,
    git_branch: Option<&str>,
    started_at: &str,
    captured_via: &str,
) -> Result<()> {
    conn.execute(
        "
        INSERT OR IGNORE INTO sessions (
            session_id,
            project_path,
            git_branch,
            started_at,
            captured_via
        ) VALUES (?1, ?2, ?3, ?4, ?5)
        ",
        params![
            session_id,
            project_path,
            git_branch,
            started_at,
            captured_via
        ],
    )?;
    Ok(())
}

pub fn upsert_prompt_session(
    conn: &Connection,
    session_id: &str,
    project_path: Option<&str>,
    git_branch: Option<&str>,
    prompt_text: &str,
    started_at: &str,
    captured_via: &str,
) -> Result<()> {
    let prompt_word_count = display::token_count(prompt_text) as i64;
    conn.execute(
        "
        INSERT INTO sessions (
            session_id,
            project_path,
            git_branch,
            started_at,
            prompt_text,
            prompt_word_count,
            total_turns,
            captured_via
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)
        ON CONFLICT(session_id) DO UPDATE SET
            project_path = COALESCE(excluded.project_path, sessions.project_path),
            git_branch = COALESCE(excluded.git_branch, sessions.git_branch),
            prompt_text = CASE
                WHEN sessions.prompt_text IS NULL OR sessions.prompt_text = ''
                    THEN excluded.prompt_text
                ELSE sessions.prompt_text
            END,
            prompt_word_count = CASE
                WHEN sessions.prompt_word_count = 0
                    THEN excluded.prompt_word_count
                ELSE sessions.prompt_word_count
            END,
            total_turns = sessions.total_turns + 1
        ",
        params![
            session_id,
            project_path,
            git_branch,
            started_at,
            prompt_text,
            prompt_word_count,
            captured_via
        ],
    )?;
    Ok(())
}

pub fn update_session_metadata(
    conn: &Connection,
    session_id: &str,
    project_path: Option<&str>,
    git_branch: Option<&str>,
    started_at: Option<&str>,
    ended_at: Option<&str>,
    prompt_text: Option<&str>,
    prompt_word_count: i64,
    total_turns: i64,
    total_tokens_in: i64,
    total_tokens_out: i64,
    outcome: &str,
    end_reason: Option<&str>,
    captured_via: &str,
) -> Result<()> {
    let started_at_value = started_at
        .map(ToOwned::to_owned)
        .unwrap_or_else(display::now_rfc3339);
    conn.execute(
        "
        INSERT INTO sessions (
            session_id,
            project_path,
            git_branch,
            started_at,
            ended_at,
            prompt_text,
            prompt_word_count,
            total_turns,
            total_tokens_in,
            total_tokens_out,
            outcome,
            end_reason,
            captured_via
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ON CONFLICT(session_id) DO UPDATE SET
            project_path = COALESCE(excluded.project_path, sessions.project_path),
            git_branch = COALESCE(excluded.git_branch, sessions.git_branch),
            started_at = COALESCE(sessions.started_at, excluded.started_at),
            ended_at = COALESCE(excluded.ended_at, sessions.ended_at),
            prompt_text = COALESCE(excluded.prompt_text, sessions.prompt_text),
            prompt_word_count = CASE
                WHEN excluded.prompt_word_count > 0 THEN excluded.prompt_word_count
                ELSE sessions.prompt_word_count
            END,
            total_turns = CASE
                WHEN excluded.total_turns > sessions.total_turns THEN excluded.total_turns
                ELSE sessions.total_turns
            END,
            total_tokens_in = CASE
                WHEN excluded.total_tokens_in > sessions.total_tokens_in
                    THEN excluded.total_tokens_in
                ELSE sessions.total_tokens_in
            END,
            total_tokens_out = CASE
                WHEN excluded.total_tokens_out > sessions.total_tokens_out
                    THEN excluded.total_tokens_out
                ELSE sessions.total_tokens_out
            END,
            outcome = excluded.outcome,
            end_reason = COALESCE(excluded.end_reason, sessions.end_reason),
            captured_via = excluded.captured_via
        ",
        params![
            session_id,
            project_path,
            git_branch,
            started_at_value,
            ended_at,
            prompt_text,
            prompt_word_count,
            total_turns,
            total_tokens_in,
            total_tokens_out,
            outcome,
            end_reason,
            captured_via,
        ],
    )?;
    Ok(())
}

pub fn insert_tool_call(
    conn: &Connection,
    session_id: &str,
    timestamp: &str,
    tool_name: &str,
    file_path: Option<&str>,
    command: Option<&str>,
    success: bool,
    error_text: Option<&str>,
) -> Result<()> {
    conn.execute(
        "
        INSERT INTO tool_calls (
            session_id,
            timestamp,
            tool_name,
            file_path,
            command,
            success,
            error_text
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            session_id,
            timestamp,
            tool_name,
            file_path,
            command,
            if success { 1 } else { 0 },
            error_text
        ],
    )?;
    Ok(())
}

pub fn stop_session(conn: &Connection, session_id: &str, ended_at: &str) -> Result<()> {
    conn.execute(
        "
        UPDATE sessions
        SET ended_at = COALESCE(ended_at, ?2)
        WHERE session_id = ?1
        ",
        params![session_id, ended_at],
    )?;
    Ok(())
}

pub fn finalize_session(
    conn: &Connection,
    session_id: &str,
    project_path: Option<&str>,
    ended_at: &str,
    outcome: &str,
    end_reason: Option<&str>,
) -> Result<()> {
    ensure_session(conn, session_id, project_path, None, ended_at, "hook")?;
    conn.execute(
        "
        UPDATE sessions
        SET project_path = COALESCE(project_path, ?2),
            ended_at = COALESCE(ended_at, ?3),
            outcome = ?4,
            end_reason = COALESCE(?5, end_reason)
        WHERE session_id = ?1
        ",
        params![session_id, project_path, ended_at, outcome, end_reason],
    )?;
    Ok(())
}

pub fn session_has_commit(conn: &Connection, session_id: &str) -> Result<bool> {
    let value: Option<String> = conn
        .query_row(
            "SELECT commit_hash FROM commits WHERE session_id = ?1 LIMIT 1",
            [session_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value.is_some())
}

#[derive(Debug, Clone, Default)]
pub struct SessionToolStats {
    pub has_edits: bool,
    pub has_reads: bool,
    pub total_tool_calls: i64,
    pub failed_tool_calls: i64,
    pub has_thrashing: bool,
}

pub fn session_tool_stats(conn: &Connection, session_id: &str) -> Result<SessionToolStats> {
    let mut stats = SessionToolStats::default();

    stats.total_tool_calls = conn.query_row(
        "SELECT COUNT(*) FROM tool_calls WHERE session_id = ?1",
        [session_id],
        |row| row.get(0),
    )?;

    stats.failed_tool_calls = conn.query_row(
        "SELECT COUNT(*) FROM tool_calls WHERE session_id = ?1 AND success = 0",
        [session_id],
        |row| row.get(0),
    )?;

    stats.has_edits = conn
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE session_id = ?1 AND tool_name IN ('Edit', 'Write', 'MultiEdit')",
            [session_id],
            |row| row.get::<_, i64>(0),
        )? > 0;

    stats.has_reads = conn
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE session_id = ?1 AND tool_name IN ('Read', 'Glob', 'Grep', 'ListDir')",
            [session_id],
            |row| row.get::<_, i64>(0),
        )? > 0;

    stats.has_thrashing = conn
        .query_row(
            "SELECT COUNT(*) FROM thrashing_events WHERE session_id = ?1",
            [session_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0) > 0;

    Ok(stats)
}

/// Classify a session outcome based on commit status and tool usage.
///
/// - `committed`   — produced a git commit
/// - `exploratory` — only read-only tools, no edits attempted
/// - `failed`      — had edits + errors or thrashing, no commit
/// - `abandoned`   — had edits but no commit, no clear failure signal
pub fn classify_outcome(has_commit: bool, stats: &SessionToolStats) -> &'static str {
    if has_commit {
        return "committed";
    }

    if !stats.has_edits {
        return "exploratory";
    }

    let failure_rate = if stats.total_tool_calls > 0 {
        stats.failed_tool_calls as f64 / stats.total_tool_calls as f64
    } else {
        0.0
    };

    if stats.has_thrashing || failure_rate > 0.3 {
        return "failed";
    }

    "abandoned"
}

pub fn insert_commit(
    conn: &Connection,
    commit_hash: &str,
    session_id: Option<&str>,
    branch: Option<&str>,
    timestamp: &str,
    files_changed_json: &str,
    insertions: i64,
    deletions: i64,
) -> Result<()> {
    conn.execute(
        "
        INSERT OR REPLACE INTO commits (
            commit_hash,
            session_id,
            branch,
            timestamp,
            files_changed,
            insertions,
            deletions
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            commit_hash,
            session_id,
            branch,
            timestamp,
            files_changed_json,
            insertions,
            deletions
        ],
    )?;
    if let Some(session_id) = session_id {
        conn.execute(
            "UPDATE sessions SET outcome = 'committed', ended_at = COALESCE(ended_at, ?2) WHERE session_id = ?1",
            params![session_id, timestamp],
        )?;
    }
    Ok(())
}

pub fn insert_edit(
    conn: &Connection,
    commit_hash: &str,
    session_id: &str,
    file_path: &str,
    ai_lines: i64,
    human_kept: i64,
    human_modified: i64,
    human_deleted: i64,
    human_added: i64,
    acceptance_rate: Option<f64>,
) -> Result<()> {
    conn.execute(
        "
        INSERT INTO edits (
            commit_hash,
            session_id,
            file_path,
            ai_lines,
            human_kept,
            human_modified,
            human_deleted,
            human_added,
            acceptance_rate
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ",
        params![
            commit_hash,
            session_id,
            file_path,
            ai_lines,
            human_kept,
            human_modified,
            human_deleted,
            human_added,
            acceptance_rate
        ],
    )?;
    Ok(())
}

pub fn insert_harness_snapshot(
    conn: &Connection,
    session_id: Option<&str>,
    file_path: &str,
    content_hash: &str,
    content: &str,
    token_count: i64,
    captured_at: &str,
) -> Result<()> {
    conn.execute(
        "
        INSERT INTO harness_snapshots (
            session_id,
            file_path,
            content_hash,
            content,
            token_count,
            captured_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ",
        params![
            session_id,
            file_path,
            content_hash,
            content,
            token_count,
            captured_at
        ],
    )?;
    Ok(())
}

pub fn insert_analysis_run(
    conn: &Connection,
    run_at: &str,
    sessions_analyzed: i64,
    harness_score: i64,
    findings_json: &str,
    generated_configs_json: &str,
) -> Result<()> {
    conn.execute(
        "
        INSERT INTO analysis_runs (
            run_at,
            sessions_analyzed,
            harness_score,
            findings_json,
            generated_configs_json
        ) VALUES (?1, ?2, ?3, ?4, ?5)
        ",
        params![
            run_at,
            sessions_analyzed,
            harness_score,
            findings_json,
            generated_configs_json
        ],
    )?;
    Ok(())
}

pub fn latest_analysis_run(conn: &Connection) -> Result<Option<AnalysisRunRecord>> {
    conn.query_row(
        "
        SELECT id, run_at, sessions_analyzed, harness_score, findings_json, generated_configs_json
        FROM analysis_runs
        ORDER BY id DESC
        LIMIT 1
        ",
        [],
        |row| {
            Ok(AnalysisRunRecord {
                id: row.get(0)?,
                run_at: row.get(1)?,
                sessions_analyzed: row.get(2)?,
                harness_score: row.get(3)?,
                findings_json: row.get(4)?,
                generated_configs_json: row.get(5)?,
            })
        },
    )
    .optional()
}

pub fn summarize_sessions(
    sessions: &[SessionRecord],
    acceptance_rate: Option<f64>,
) -> ProjectSummary {
    let total_sessions = sessions.len();
    let committed = sessions
        .iter()
        .filter(|session| session.outcome == "committed")
        .count();
    let abandoned = sessions
        .iter()
        .filter(|session| session.outcome == "abandoned")
        .count();
    let failed = sessions
        .iter()
        .filter(|session| session.outcome == "failed")
        .count();
    let exploratory = sessions
        .iter()
        .filter(|session| session.outcome == "exploratory")
        .count();
    let in_progress = sessions
        .iter()
        .filter(|session| session.outcome == "in_progress")
        .count();
    let total_turns = sessions
        .iter()
        .map(|session| session.total_turns)
        .sum::<i64>();
    let total_tokens_in = sessions
        .iter()
        .map(|session| session.total_tokens_in)
        .sum::<i64>();
    let total_tokens_out = sessions
        .iter()
        .map(|session| session.total_tokens_out)
        .sum::<i64>();
    let min_started_at = sessions
        .iter()
        .map(|session| session.started_at.as_str())
        .min();
    let max_started_at = sessions
        .iter()
        .map(|session| session.started_at.as_str())
        .max();

    ProjectSummary {
        total_sessions,
        committed,
        abandoned,
        failed,
        exploratory,
        in_progress,
        avg_turns: if total_sessions == 0 {
            0.0
        } else {
            total_turns as f64 / total_sessions as f64
        },
        total_tokens_in,
        total_tokens_out,
        min_started_at: min_started_at.map(ToOwned::to_owned),
        max_started_at: max_started_at.map(ToOwned::to_owned),
        acceptance_rate,
    }
}

pub fn load_scope_breakdown(
    conn: &Connection,
    current_project_path: &str,
    since: &str,
) -> Result<ScopeBreakdown> {
    let sessions = load_sessions_for_scope(conn, current_project_path, since, AnalysisScope::User)?;
    let mut current_project_sessions = 0_usize;
    let mut other_project_sessions = 0_usize;
    let mut project_paths = HashSet::new();

    for session in sessions {
        if let Some(project_path) = session.project_path {
            project_paths.insert(project_path.clone());
            if project_path == current_project_path {
                current_project_sessions += 1;
            } else {
                other_project_sessions += 1;
            }
        } else {
            other_project_sessions += 1;
        }
    }

    Ok(ScopeBreakdown {
        current_project_sessions,
        other_project_sessions,
        project_count: project_paths.len(),
    })
}

pub fn load_summary_for_scope(
    conn: &Connection,
    project_path: &str,
    since: &str,
    scope: AnalysisScope,
) -> Result<ProjectSummary> {
    let sessions = load_sessions_for_scope(conn, project_path, since, scope)?;
    let acceptance_rate = load_acceptance_rate_for_scope(conn, project_path, since, scope)?;
    Ok(summarize_sessions(&sessions, acceptance_rate))
}

pub fn load_acceptance_rate_for_scope(
    conn: &Connection,
    project_path: &str,
    since: &str,
    scope: AnalysisScope,
) -> Result<Option<f64>> {
    let sql = match scope {
        AnalysisScope::Project => {
            "
            SELECT AVG(edits.acceptance_rate)
            FROM edits
            JOIN sessions ON sessions.session_id = edits.session_id
            WHERE sessions.project_path = ?1
              AND sessions.started_at >= ?2
            "
        }
        AnalysisScope::User | AnalysisScope::Both => {
            "
            SELECT AVG(edits.acceptance_rate)
            FROM edits
            JOIN sessions ON sessions.session_id = edits.session_id
            WHERE sessions.started_at >= ?1
            "
        }
    };

    match scope {
        AnalysisScope::Project => {
            conn.query_row(sql, params![project_path, since], |row| row.get(0))
        }
        AnalysisScope::User | AnalysisScope::Both => {
            conn.query_row(sql, params![since], |row| row.get(0))
        }
    }
}

pub fn load_sessions_for_scope(
    conn: &Connection,
    project_path: &str,
    since: &str,
    scope: AnalysisScope,
) -> Result<Vec<SessionRecord>> {
    let sql = match scope {
        AnalysisScope::Project => {
            "
            SELECT
                session_id,
                project_path,
                git_branch,
                started_at,
                ended_at,
                prompt_text,
                prompt_word_count,
                total_turns,
                total_tokens_in,
                total_tokens_out,
                outcome,
                end_reason,
                captured_via
            FROM sessions
            WHERE project_path = ?1
              AND started_at >= ?2
            ORDER BY started_at DESC
            "
        }
        AnalysisScope::User | AnalysisScope::Both => {
            "
            SELECT
                session_id,
                project_path,
                git_branch,
                started_at,
                ended_at,
                prompt_text,
                prompt_word_count,
                total_turns,
                total_tokens_in,
                total_tokens_out,
                outcome,
                end_reason,
                captured_via
            FROM sessions
            WHERE started_at >= ?1
            ORDER BY started_at DESC
            "
        }
    };

    let mut statement = conn.prepare(sql)?;
    let rows = match scope {
        AnalysisScope::Project => {
            statement.query_map(params![project_path, since], map_session_row)?
        }
        AnalysisScope::User | AnalysisScope::Both => {
            statement.query_map(params![since], map_session_row)?
        }
    };

    rows.collect()
}

pub fn load_tool_calls_for_scope(
    conn: &Connection,
    project_path: &str,
    since: &str,
    scope: AnalysisScope,
) -> Result<Vec<ToolCallRecord>> {
    let sql = match scope {
        AnalysisScope::Project => {
            "
            SELECT
                tool_calls.session_id,
                tool_calls.timestamp,
                tool_calls.tool_name,
                tool_calls.file_path,
                tool_calls.command,
                tool_calls.success,
                tool_calls.error_text
            FROM tool_calls
            JOIN sessions ON sessions.session_id = tool_calls.session_id
            WHERE sessions.project_path = ?1
              AND sessions.started_at >= ?2
            ORDER BY tool_calls.timestamp DESC
            "
        }
        AnalysisScope::User | AnalysisScope::Both => {
            "
            SELECT
                tool_calls.session_id,
                tool_calls.timestamp,
                tool_calls.tool_name,
                tool_calls.file_path,
                tool_calls.command,
                tool_calls.success,
                tool_calls.error_text
            FROM tool_calls
            JOIN sessions ON sessions.session_id = tool_calls.session_id
            WHERE sessions.started_at >= ?1
            ORDER BY tool_calls.timestamp DESC
            "
        }
    };

    let mut statement = conn.prepare(sql)?;
    let rows = match scope {
        AnalysisScope::Project => {
            statement.query_map(params![project_path, since], map_tool_call_row)?
        }
        AnalysisScope::User | AnalysisScope::Both => {
            statement.query_map(params![since], map_tool_call_row)?
        }
    };

    rows.collect()
}

pub fn load_project_summary(
    conn: &Connection,
    project_path: &str,
    since: &str,
) -> Result<ProjectSummary> {
    load_summary_for_scope(conn, project_path, since, AnalysisScope::Project)
}

pub fn recent_candidate_sessions(
    conn: &Connection,
    project_path: &str,
    since: &str,
) -> Result<Vec<SessionRecord>> {
    let mut statement = conn.prepare(
        "
        SELECT
            session_id,
            project_path,
            git_branch,
            started_at,
            ended_at,
            prompt_text,
            prompt_word_count,
            total_turns,
            total_tokens_in,
            total_tokens_out,
            outcome,
            end_reason,
            captured_via
        FROM sessions
        WHERE project_path = ?1
          AND started_at >= ?2
          AND outcome = 'in_progress'
        ORDER BY started_at DESC
        ",
    )?;

    let rows = statement.query_map(params![project_path, since], |row| {
        Ok(SessionRecord {
            session_id: row.get(0)?,
            project_path: row.get(1)?,
            git_branch: row.get(2)?,
            started_at: row.get(3)?,
            ended_at: row.get(4)?,
            prompt_text: row.get(5)?,
            prompt_word_count: row.get(6)?,
            total_turns: row.get(7)?,
            total_tokens_in: row.get(8)?,
            total_tokens_out: row.get(9)?,
            outcome: row.get(10)?,
            end_reason: row.get(11)?,
            captured_via: row.get(12)?,
        })
    })?;

    rows.collect()
}

pub fn insert_session_event(
    conn: &Connection,
    session_id: &str,
    timestamp: &str,
    event_type: &str,
    detail: Option<&str>,
) -> Result<()> {
    conn.execute(
        "
        INSERT INTO session_events (
            session_id,
            timestamp,
            event_type,
            detail
        ) VALUES (?1, ?2, ?3, ?4)
        ",
        params![session_id, timestamp, event_type, detail],
    )?;
    Ok(())
}

pub fn session_file_paths(conn: &Connection, session_id: &str) -> Result<Vec<String>> {
    let mut statement = conn.prepare(
        "
        SELECT DISTINCT file_path
        FROM tool_calls
        WHERE session_id = ?1
          AND file_path IS NOT NULL
        ",
    )?;

    let rows = statement.query_map([session_id], |row| row.get::<_, String>(0))?;
    rows.collect()
}

pub fn insert_thrashing_event(
    conn: &Connection,
    session_id: &str,
    thrash_type: &str,
    target: &str,
    cycle_count: i64,
    first_seen: &str,
    last_seen: &str,
) -> Result<()> {
    conn.execute(
        "
        INSERT INTO thrashing_events (
            session_id, thrash_type, target, cycle_count, first_seen, last_seen
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ",
        params![session_id, thrash_type, target, cycle_count, first_seen, last_seen],
    )?;
    Ok(())
}

pub fn load_thrashing_for_scope(
    conn: &Connection,
    project_path: &str,
    since: &str,
    scope: AnalysisScope,
) -> Result<Vec<ThrashingRecord>> {
    let sql = match scope {
        AnalysisScope::Project => {
            "
            SELECT t.session_id, t.thrash_type, t.target, t.cycle_count, t.first_seen, t.last_seen
            FROM thrashing_events t
            JOIN sessions s ON s.session_id = t.session_id
            WHERE s.project_path = ?1 AND s.started_at >= ?2
            ORDER BY t.cycle_count DESC
            "
        }
        AnalysisScope::User | AnalysisScope::Both => {
            "
            SELECT t.session_id, t.thrash_type, t.target, t.cycle_count, t.first_seen, t.last_seen
            FROM thrashing_events t
            JOIN sessions s ON s.session_id = t.session_id
            WHERE s.started_at >= ?1
            ORDER BY t.cycle_count DESC
            "
        }
    };

    let mut statement = conn.prepare(sql)?;
    let rows = match scope {
        AnalysisScope::Project => {
            statement.query_map(params![project_path, since], map_thrashing_row)?
        }
        AnalysisScope::User | AnalysisScope::Both => {
            statement.query_map(params![since], map_thrashing_row)?
        }
    };

    rows.collect()
}

pub fn insert_prompt_turn(
    conn: &Connection,
    session_id: &str,
    turn_number: i64,
    timestamp: &str,
    prompt_text: &str,
    word_count: i64,
    classification: &str,
) -> Result<()> {
    conn.execute(
        "
        INSERT INTO prompt_turns (
            session_id, turn_number, timestamp, prompt_text, word_count, classification
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ",
        params![session_id, turn_number, timestamp, prompt_text, word_count, classification],
    )?;
    Ok(())
}

pub fn session_turn_count(conn: &Connection, session_id: &str) -> Result<i64> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM prompt_turns WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(count)
}

pub fn load_prompt_turns_for_scope(
    conn: &Connection,
    project_path: &str,
    since: &str,
    scope: AnalysisScope,
) -> Result<Vec<PromptTurnRecord>> {
    let sql = match scope {
        AnalysisScope::Project => {
            "
            SELECT p.session_id, p.turn_number, p.timestamp, p.prompt_text, p.word_count, p.classification
            FROM prompt_turns p
            JOIN sessions s ON s.session_id = p.session_id
            WHERE s.project_path = ?1 AND s.started_at >= ?2
            ORDER BY p.session_id, p.turn_number
            "
        }
        AnalysisScope::User | AnalysisScope::Both => {
            "
            SELECT p.session_id, p.turn_number, p.timestamp, p.prompt_text, p.word_count, p.classification
            FROM prompt_turns p
            JOIN sessions s ON s.session_id = p.session_id
            WHERE s.started_at >= ?1
            ORDER BY p.session_id, p.turn_number
            "
        }
    };

    let mut statement = conn.prepare(sql)?;
    let rows = match scope {
        AnalysisScope::Project => {
            statement.query_map(params![project_path, since], map_prompt_turn_row)?
        }
        AnalysisScope::User | AnalysisScope::Both => {
            statement.query_map(params![since], map_prompt_turn_row)?
        }
    };

    rows.collect()
}

fn map_session_row(row: &rusqlite::Row<'_>) -> Result<SessionRecord> {
    Ok(SessionRecord {
        session_id: row.get(0)?,
        project_path: row.get(1)?,
        git_branch: row.get(2)?,
        started_at: row.get(3)?,
        ended_at: row.get(4)?,
        prompt_text: row.get(5)?,
        prompt_word_count: row.get(6)?,
        total_turns: row.get(7)?,
        total_tokens_in: row.get(8)?,
        total_tokens_out: row.get(9)?,
        outcome: row.get(10)?,
        end_reason: row.get(11)?,
        captured_via: row.get(12)?,
    })
}

fn map_tool_call_row(row: &rusqlite::Row<'_>) -> Result<ToolCallRecord> {
    Ok(ToolCallRecord {
        session_id: row.get(0)?,
        timestamp: row.get(1)?,
        tool_name: row.get(2)?,
        file_path: row.get(3)?,
        command: row.get(4)?,
        success: row.get::<_, i64>(5)? == 1,
        error_text: row.get(6)?,
    })
}

fn map_thrashing_row(row: &rusqlite::Row<'_>) -> Result<ThrashingRecord> {
    Ok(ThrashingRecord {
        session_id: row.get(0)?,
        thrash_type: row.get(1)?,
        target: row.get(2)?,
        cycle_count: row.get(3)?,
        first_seen: row.get(4)?,
        last_seen: row.get(5)?,
    })
}

fn map_prompt_turn_row(row: &rusqlite::Row<'_>) -> Result<PromptTurnRecord> {
    Ok(PromptTurnRecord {
        session_id: row.get(0)?,
        turn_number: row.get(1)?,
        timestamp: row.get(2)?,
        prompt_text: row.get(3)?,
        word_count: row.get(4)?,
        classification: row.get(5)?,
    })
}
