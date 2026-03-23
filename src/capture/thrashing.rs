use crate::db::queries;
use rusqlite::{Connection, Result};
use std::collections::HashMap;

/// Scan a session's tool calls for thrashing patterns and store results.
///
/// File cycles: same file read then edited 2+ times.
/// Bash retries: same base command executed 3+ times.
pub fn detect_and_store(conn: &Connection, session_id: &str) -> Result<()> {
    let tool_calls = load_session_tool_calls(conn, session_id)?;
    if tool_calls.is_empty() {
        return Ok(());
    }

    detect_file_cycles(conn, session_id, &tool_calls)?;
    detect_bash_retries(conn, session_id, &tool_calls)?;
    Ok(())
}

struct ToolEntry {
    timestamp: String,
    tool_name: String,
    file_path: Option<String>,
    command: Option<String>,
}

fn load_session_tool_calls(conn: &Connection, session_id: &str) -> Result<Vec<ToolEntry>> {
    let mut stmt = conn.prepare(
        "SELECT timestamp, tool_name, file_path, command
         FROM tool_calls
         WHERE session_id = ?1
         ORDER BY timestamp ASC",
    )?;

    let rows = stmt.query_map([session_id], |row| {
        Ok(ToolEntry {
            timestamp: row.get(0)?,
            tool_name: row.get(1)?,
            file_path: row.get(2)?,
            command: row.get(3)?,
        })
    })?;

    rows.collect()
}

fn is_read_op(tool_name: &str) -> bool {
    matches!(tool_name, "Read" | "Glob" | "Grep" | "ListDir")
}

fn is_write_op(tool_name: &str) -> bool {
    matches!(tool_name, "Edit" | "Write" | "MultiEdit")
}

fn detect_file_cycles(
    conn: &Connection,
    session_id: &str,
    tool_calls: &[ToolEntry],
) -> Result<()> {
    // Track read->write transitions per file path
    // State machine per file: None -> Read -> Write -> Read (cycle!) -> Write (cycle!) ...
    #[derive(Default)]
    struct FileState {
        state: u8, // 0=init, 1=read, 2=written
        cycles: i64,
        first_seen: Option<String>,
        last_seen: Option<String>,
    }

    let mut files: HashMap<String, FileState> = HashMap::new();

    for entry in tool_calls {
        let path = match entry.file_path.as_deref() {
            Some(p) if !p.is_empty() => p,
            _ => continue,
        };

        if !is_read_op(&entry.tool_name) && !is_write_op(&entry.tool_name) {
            continue;
        }

        let state = files.entry(path.to_string()).or_default();
        if state.first_seen.is_none() {
            state.first_seen = Some(entry.timestamp.clone());
        }
        state.last_seen = Some(entry.timestamp.clone());

        if is_read_op(&entry.tool_name) {
            if state.state == 2 {
                // We wrote and now we're reading again — potential cycle start
                state.state = 1;
            } else {
                state.state = 1;
            }
        } else if is_write_op(&entry.tool_name) && state.state == 1 {
            if state.cycles > 0 || state.state == 1 {
                // First write after read is normal (cycle 0), subsequent read->write = thrashing
                if state.cycles > 0 {
                    // Already had a full cycle, this is another
                }
            }
            state.state = 2;
            state.cycles += 1;
        }
    }

    for (path, state) in &files {
        // cycles counts total read->write transitions. The first one is normal.
        // Thrashing starts at 2+ transitions (i.e. cycles >= 2).
        if state.cycles >= 2 {
            queries::insert_thrashing_event(
                conn,
                session_id,
                "file_cycle",
                path,
                state.cycles - 1, // subtract the initial normal read->edit
                state.first_seen.as_deref().unwrap_or(""),
                state.last_seen.as_deref().unwrap_or(""),
            )?;
        }
    }

    Ok(())
}

fn normalize_command(cmd: &str) -> String {
    // Take first two tokens as the "base command"
    let tokens: Vec<&str> = cmd.split_whitespace().take(2).collect();
    tokens.join(" ").to_lowercase()
}

fn detect_bash_retries(
    conn: &Connection,
    session_id: &str,
    tool_calls: &[ToolEntry],
) -> Result<()> {
    #[derive(Default)]
    struct CmdState {
        count: i64,
        first_seen: Option<String>,
        last_seen: Option<String>,
    }

    let mut commands: HashMap<String, CmdState> = HashMap::new();

    for entry in tool_calls {
        if entry.tool_name != "Bash" {
            continue;
        }
        let cmd = match entry.command.as_deref() {
            Some(c) if !c.is_empty() => c,
            _ => continue,
        };

        let key = normalize_command(cmd);
        if key.is_empty() {
            continue;
        }

        let state = commands.entry(key).or_default();
        if state.first_seen.is_none() {
            state.first_seen = Some(entry.timestamp.clone());
        }
        state.last_seen = Some(entry.timestamp.clone());
        state.count += 1;
    }

    for (cmd, state) in &commands {
        if state.count >= 3 {
            queries::insert_thrashing_event(
                conn,
                session_id,
                "bash_retry",
                cmd,
                state.count,
                state.first_seen.as_deref().unwrap_or(""),
                state.last_seen.as_deref().unwrap_or(""),
            )?;
        }
    }

    Ok(())
}
