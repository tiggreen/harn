use crate::db::{self, queries};
use crate::display;
use crate::AppResult;
use serde::Deserialize;
use std::io::Read;

#[derive(Debug, Deserialize)]
struct PostCompactInput {
    session_id: String,
    cwd: Option<String>,
    #[serde(default)]
    summary: Option<String>,
}

pub fn handle_from_stdin() -> AppResult<()> {
    let input = read_input()?;
    let conn = db::connect()?;
    let now = display::now_rfc3339();

    queries::ensure_session(&conn, &input.session_id, input.cwd.as_deref(), None, &now, "hook")?;

    queries::insert_session_event(
        &conn,
        &input.session_id,
        &now,
        "compaction",
        input.summary.as_deref(),
    )?;

    Ok(())
}

fn read_input() -> AppResult<PostCompactInput> {
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    Ok(serde_json::from_str(&buffer)?)
}
