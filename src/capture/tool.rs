use crate::db::{self, queries};
use crate::display;
use crate::AppResult;
use serde::Deserialize;
use serde_json::Value;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct ToolInput {
    session_id: String,
    tool_name: String,
    #[serde(default)]
    tool_input: Value,
    #[serde(default)]
    tool_response: Value,
}

pub fn handle_from_stdin() -> AppResult<()> {
    let input = read_input()?;
    let conn = db::connect()?;
    queries::ensure_session(
        &conn,
        &input.session_id,
        None,
        None,
        &display::now_rfc3339(),
        "hook",
    )?;

    let project_path = queries::session_project_path(&conn, &input.session_id)?;
    let file_path = extract_file_path(&input.tool_input)
        .map(|path| display::normalize_file_path(&path, project_path.as_deref().map(Path::new)));
    let command = extract_command(&input.tool_input);
    let success = infer_success(&input.tool_name, &input.tool_response);
    let error_text = infer_error_text(&input.tool_response);

    queries::insert_tool_call(
        &conn,
        &input.session_id,
        &display::now_rfc3339(),
        &input.tool_name,
        file_path.as_deref(),
        command.as_deref(),
        success,
        error_text.as_deref(),
    )?;

    Ok(())
}

fn read_input() -> AppResult<ToolInput> {
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    Ok(serde_json::from_str(&buffer)?)
}

fn extract_file_path(tool_input: &Value) -> Option<String> {
    tool_input
        .get("file_path")
        .and_then(Value::as_str)
        .or_else(|| tool_input.get("path").and_then(Value::as_str))
        .or_else(|| {
            tool_input
                .get("edits")
                .and_then(Value::as_array)
                .and_then(|edits| edits.first())
                .and_then(|first| first.get("file_path"))
                .and_then(Value::as_str)
        })
        .map(ToOwned::to_owned)
}

fn extract_command(tool_input: &Value) -> Option<String> {
    tool_input
        .get("command")
        .and_then(Value::as_str)
        .or_else(|| tool_input.get("cmd").and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

fn infer_success(tool_name: &str, tool_response: &Value) -> bool {
    if let Some(value) = tool_response.get("success").and_then(Value::as_bool) {
        return value;
    }
    if tool_name == "Bash" {
        if let Some(exit_code) = tool_response
            .get("exit_code")
            .or_else(|| tool_response.get("exitCode"))
            .and_then(Value::as_i64)
        {
            return exit_code == 0;
        }
    }
    !tool_response.get("error").is_some()
}

fn infer_error_text(tool_response: &Value) -> Option<String> {
    tool_response
        .get("error")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            tool_response
                .get("stderr")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}
