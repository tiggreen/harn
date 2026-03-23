use rusqlite::{params, Connection, Result};

pub fn init(conn: &mut Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=OFF;")?;
    migrate_legacy_schema(conn)?;
    create_tables(conn)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(())
}

fn migrate_legacy_schema(conn: &mut Connection) -> Result<()> {
    let mut sessions_legacy = false;
    let mut tool_calls_legacy = false;
    let mut commits_legacy = false;
    let mut edits_legacy = false;
    let mut harness_snapshots_legacy = false;
    let mut analysis_runs_legacy = false;

    if table_exists(conn, "sessions")? && !has_column(conn, "sessions", "session_id")? {
        rename_table(conn, "sessions", "sessions_legacy")?;
        sessions_legacy = true;
    }

    if table_exists(conn, "tool_calls")? && !has_column(conn, "tool_calls", "file_path")? {
        rename_table(conn, "tool_calls", "tool_calls_legacy")?;
        tool_calls_legacy = true;
    }

    if table_exists(conn, "commits")? && !has_column(conn, "commits", "commit_hash")? {
        rename_table(conn, "commits", "commits_legacy")?;
        commits_legacy = true;
    }

    if table_exists(conn, "edits")? && !has_column(conn, "edits", "commit_hash")? {
        rename_table(conn, "edits", "edits_legacy")?;
        edits_legacy = true;
    }

    if table_exists(conn, "harness_snapshots")?
        && column_notnull(conn, "harness_snapshots", "session_id")?
    {
        rename_table(conn, "harness_snapshots", "harness_snapshots_legacy")?;
        harness_snapshots_legacy = true;
    }

    if table_exists(conn, "analysis_runs")? && has_column(conn, "analysis_runs", "period_start")? {
        rename_table(conn, "analysis_runs", "analysis_runs_legacy")?;
        analysis_runs_legacy = true;
    }

    if !(sessions_legacy
        || tool_calls_legacy
        || commits_legacy
        || edits_legacy
        || harness_snapshots_legacy
        || analysis_runs_legacy)
    {
        return Ok(());
    }

    create_tables(conn)?;

    if sessions_legacy {
        conn.execute_batch(
            "
            INSERT OR IGNORE INTO sessions (
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
            )
            SELECT
                id,
                cwd,
                git_branch,
                started_at,
                ended_at,
                prompt_text,
                COALESCE(prompt_word_count, 0),
                COALESCE(total_turns, 0),
                COALESCE(total_tokens_in, 0),
                COALESCE(total_tokens_out, 0),
                COALESCE(outcome, 'in_progress'),
                notes,
                'legacy'
            FROM sessions_legacy;
            ",
        )?;
        conn.execute_batch("DROP TABLE sessions_legacy;")?;
    }

    if tool_calls_legacy {
        conn.execute_batch(
            "
            INSERT INTO tool_calls (
                session_id,
                timestamp,
                tool_name,
                file_path,
                command,
                success,
                error_text
            )
            SELECT
                session_id,
                timestamp,
                tool_name,
                input_file,
                input_command,
                COALESCE(success, 1),
                error_text
            FROM tool_calls_legacy;
            ",
        )?;
        conn.execute_batch("DROP TABLE tool_calls_legacy;")?;
    }

    if commits_legacy {
        conn.execute_batch(
            "
            INSERT OR IGNORE INTO commits (
                commit_hash,
                session_id,
                branch,
                timestamp,
                files_changed,
                insertions,
                deletions
            )
            SELECT
                id,
                session_id,
                NULL,
                timestamp,
                files_changed,
                COALESCE(insertions, 0),
                COALESCE(deletions, 0)
            FROM commits_legacy;
            ",
        )?;
        conn.execute_batch("DROP TABLE commits_legacy;")?;
    }

    if edits_legacy {
        conn.execute_batch(
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
            )
            SELECT
                commit_id,
                session_id,
                file_path,
                COALESCE(ai_lines, 0),
                COALESCE(human_kept, 0),
                COALESCE(human_modified, 0),
                COALESCE(human_deleted, 0),
                COALESCE(human_added, 0),
                acceptance_rate
            FROM edits_legacy;
            ",
        )?;
        conn.execute_batch("DROP TABLE edits_legacy;")?;
    }

    if harness_snapshots_legacy {
        conn.execute_batch(
            "
            INSERT INTO harness_snapshots (
                session_id,
                file_path,
                content_hash,
                content,
                token_count,
                captured_at
            )
            SELECT
                session_id,
                file_path,
                content_hash,
                content,
                token_count,
                captured_at
            FROM harness_snapshots_legacy;
            ",
        )?;
        conn.execute_batch("DROP TABLE harness_snapshots_legacy;")?;
    }

    if analysis_runs_legacy {
        conn.execute(
            "
            INSERT INTO analysis_runs (
                run_at,
                sessions_analyzed,
                harness_score,
                findings_json,
                generated_configs_json
            )
            SELECT
                run_at,
                sessions_analyzed,
                harness_score,
                findings_json,
                generated_configs_json
            FROM analysis_runs_legacy
            ORDER BY id ASC
            ",
            params![],
        )?;
        conn.execute_batch("DROP TABLE analysis_runs_legacy;")?;
    }

    Ok(())
}

fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS sessions (
            session_id TEXT PRIMARY KEY,
            project_path TEXT,
            git_branch TEXT,
            started_at TEXT NOT NULL,
            ended_at TEXT,
            prompt_text TEXT,
            prompt_word_count INTEGER DEFAULT 0,
            total_turns INTEGER DEFAULT 0,
            total_tokens_in INTEGER DEFAULT 0,
            total_tokens_out INTEGER DEFAULT 0,
            outcome TEXT DEFAULT 'in_progress',
            end_reason TEXT,
            captured_via TEXT DEFAULT 'hook'
        );

        CREATE TABLE IF NOT EXISTS tool_calls (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            tool_name TEXT NOT NULL,
            file_path TEXT,
            command TEXT,
            success INTEGER DEFAULT 1,
            error_text TEXT,
            FOREIGN KEY (session_id) REFERENCES sessions(session_id)
        );

        CREATE TABLE IF NOT EXISTS commits (
            commit_hash TEXT PRIMARY KEY,
            session_id TEXT,
            branch TEXT,
            timestamp TEXT NOT NULL,
            files_changed TEXT,
            insertions INTEGER DEFAULT 0,
            deletions INTEGER DEFAULT 0,
            FOREIGN KEY (session_id) REFERENCES sessions(session_id)
        );

        CREATE TABLE IF NOT EXISTS edits (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            commit_hash TEXT NOT NULL,
            session_id TEXT NOT NULL,
            file_path TEXT NOT NULL,
            ai_lines INTEGER DEFAULT 0,
            human_kept INTEGER DEFAULT 0,
            human_modified INTEGER DEFAULT 0,
            human_deleted INTEGER DEFAULT 0,
            human_added INTEGER DEFAULT 0,
            acceptance_rate REAL,
            FOREIGN KEY (commit_hash) REFERENCES commits(commit_hash)
        );

        CREATE TABLE IF NOT EXISTS harness_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT,
            file_path TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            content TEXT NOT NULL,
            token_count INTEGER,
            captured_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS analysis_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_at TEXT NOT NULL,
            sessions_analyzed INTEGER,
            harness_score INTEGER,
            findings_json TEXT,
            generated_configs_json TEXT
        );

        CREATE TABLE IF NOT EXISTS session_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            event_type TEXT NOT NULL,
            detail TEXT,
            FOREIGN KEY (session_id) REFERENCES sessions(session_id)
        );

        CREATE INDEX IF NOT EXISTS idx_tool_calls_session_id
            ON tool_calls(session_id);
        CREATE INDEX IF NOT EXISTS idx_sessions_started_at
            ON sessions(started_at);
        CREATE INDEX IF NOT EXISTS idx_sessions_outcome
            ON sessions(outcome);
        CREATE INDEX IF NOT EXISTS idx_commits_session_id
            ON commits(session_id);
        CREATE INDEX IF NOT EXISTS idx_edits_session_id
            ON edits(session_id);
        CREATE INDEX IF NOT EXISTS idx_session_events_session_id
            ON session_events(session_id);

        CREATE TABLE IF NOT EXISTS thrashing_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            thrash_type TEXT NOT NULL,
            target TEXT NOT NULL,
            cycle_count INTEGER NOT NULL,
            first_seen TEXT NOT NULL,
            last_seen TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(session_id)
        );

        CREATE INDEX IF NOT EXISTS idx_thrashing_session_id
            ON thrashing_events(session_id);

        CREATE TABLE IF NOT EXISTS prompt_turns (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            turn_number INTEGER NOT NULL,
            timestamp TEXT NOT NULL,
            prompt_text TEXT NOT NULL,
            word_count INTEGER NOT NULL,
            classification TEXT NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(session_id)
        );

        CREATE INDEX IF NOT EXISTS idx_prompt_turns_session_id
            ON prompt_turns(session_id);
        ",
    )?;

    Ok(())
}

fn table_exists(conn: &Connection, table_name: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table_name],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn has_column(conn: &Connection, table_name: &str, column_name: &str) -> Result<bool> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table_name})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column_name {
            return Ok(true);
        }
    }
    Ok(false)
}

fn column_notnull(conn: &Connection, table_name: &str, column_name: &str) -> Result<bool> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table_name})"))?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)?))
    })?;

    for row in rows {
        let (name, notnull) = row?;
        if name == column_name {
            return Ok(notnull == 1);
        }
    }

    Ok(false)
}

fn rename_table(conn: &Connection, old_name: &str, new_name: &str) -> Result<()> {
    conn.execute_batch(&format!(
        "DROP TABLE IF EXISTS {new_name}; ALTER TABLE {old_name} RENAME TO {new_name};"
    ))?;
    Ok(())
}
