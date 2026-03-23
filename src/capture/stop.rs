use crate::db::{self, queries};
use crate::display;
use crate::AppResult;
use serde::Deserialize;
use std::io::Read;

#[derive(Debug, Deserialize)]
struct StopInput {
    session_id: String,
    cwd: Option<String>,
    transcript_path: Option<String>,
}

pub fn handle_from_stdin() -> AppResult<()> {
    let input = read_input()?;
    let conn = db::connect()?;
    let _ = input.cwd;
    let _ = input.transcript_path;
    queries::ensure_session(
        &conn,
        &input.session_id,
        None,
        None,
        &display::now_rfc3339(),
        "hook",
    )?;
    queries::stop_session(&conn, &input.session_id, &display::now_rfc3339())?;
    Ok(())
}

fn read_input() -> AppResult<StopInput> {
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    Ok(serde_json::from_str(&buffer)?)
}
