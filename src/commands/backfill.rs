use crate::capture::backfill;
use crate::config::HarnConfig;
use crate::db;
use crate::AppResult;

pub fn run(days: i64) -> AppResult<()> {
    let config = HarnConfig::load();
    let conn = db::connect()?;
    let current_project_path = crate::display::current_project_path()
        .ok()
        .map(|path| path.to_string_lossy().to_string());
    let stats = backfill::run_backfill(&conn, days, &config, current_project_path.as_deref())?;

    println!("Backfill complete.");
    println!("Imported {} sessions.", stats.imported_sessions);
    println!(
        "Current project: {} imported, {} skipped.",
        stats.imported_current_project_sessions, stats.skipped_current_project_sessions
    );
    println!(
        "Other projects: {} imported, {} skipped.",
        stats.imported_other_project_sessions, stats.skipped_other_project_sessions
    );
    println!("Imported {} tool calls.", stats.imported_tool_calls);
    if stats.skipped_sessions > 0 {
        println!(
            "Skipped {} sessions already in the database.",
            stats.skipped_sessions
        );
    }

    Ok(())
}
