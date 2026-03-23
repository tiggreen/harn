use crate::capture::{backfill, session_end};
use crate::config::HarnConfig;
use crate::db::{self, queries};
use crate::display;
use crate::AppResult;
use chrono::{Duration, Utc};
use serde_json::{json, Map, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn run() -> AppResult<()> {
    let project_path = display::current_project_path()?;
    let project_path_string = project_path.to_string_lossy().to_string();
    let conn = db::connect()?;

    let config_created = ensure_config()?;
    let claude_found = find_claude_project_dir(&project_path);
    let stack = detect_stack(&project_path);
    let agents_path = project_path.join("AGENTS.md");
    let agents_content = fs::read_to_string(&agents_path).ok();
    let commands_dir = project_path.join(".claude").join("commands");
    let custom_commands = list_custom_commands(&commands_dir)?;

    install_claude_hooks(&project_path)?;
    install_git_hook(&project_path)?;
    let snapshot_count = session_end::snapshot_harness_files(&conn, &project_path, None)?;

    let config = HarnConfig::load();
    let backfill_stats = backfill::run_backfill(&conn, 30, &config, Some(&project_path_string))?;
    let since = (Utc::now() - Duration::days(30)).to_rfc3339();
    let summary = queries::load_project_summary(&conn, &project_path_string, &since)?;
    let user_breakdown = queries::load_scope_breakdown(&conn, &project_path_string, &since)?;

    println!("  harn v0.1.0");
    println!();
    println!("  [ok] Stack detected      {stack}");
    if let Some(path) = claude_found {
        println!("  [ok] Claude Code found   {}", path.display());
    } else {
        println!("  [..] Claude Code found   ~/.claude/projects/");
    }

    if let Some(content) = agents_content.as_deref() {
        println!(
            "  [ok] AGENTS.md found     {} rules, {} tokens",
            display::count_rules(content),
            display::token_count(content)
        );
    } else {
        println!("  [..] AGENTS.md found     not found yet");
    }

    if custom_commands.is_empty() {
        println!("  [..] Custom commands     none found");
    } else {
        println!(
            "  [ok] Custom commands     {} found: {}",
            custom_commands.len(),
            custom_commands.join(", ")
        );
    }
    println!("  [ok] Hooks installed     .claude/settings.json");
    println!("  [ok] Git hook installed  .git/hooks/post-commit");
    if config_created {
        println!(
            "  [ok] Config created      {}",
            HarnConfig::path().display()
        );
    } else {
        println!(
            "  [ok] Config ready        {}",
            HarnConfig::path().display()
        );
    }
    println!(
        "  [ok] Backfill complete   {} sessions imported (last 30 days)",
        backfill_stats.imported_sessions
    );
    println!(
        "       This project        {} imported, {} already known",
        backfill_stats.imported_current_project_sessions,
        backfill_stats.skipped_current_project_sessions
    );
    println!(
        "       Other projects      {} imported, {} already known",
        backfill_stats.imported_other_project_sessions,
        backfill_stats.skipped_other_project_sessions
    );
    if snapshot_count > 0 {
        println!("  [ok] Snapshot saved      {snapshot_count} harness files captured");
    }

    println!();
    println!("  Quick look at your data:");
    println!(
        "    Sessions: {} ({} committed, {} abandoned, {} failed, {} exploratory, {} in progress)",
        summary.total_sessions, summary.committed, summary.abandoned,
        summary.failed, summary.exploratory, summary.in_progress
    );
    println!("    Avg turns: {:.1}", summary.avg_turns);
    if summary.total_sessions > 0 {
        let actionable = summary.committed + summary.abandoned + summary.failed;
        let failure_count = summary.abandoned + summary.failed;
        if actionable > 0 {
            println!(
                "    Failure rate: {}% ({} abandoned + {} failed out of {} actionable sessions)",
                display::percentage(failure_count, actionable),
                summary.abandoned, summary.failed, actionable
            );
        }
    } else {
        println!("    Failure rate: no sessions for this project yet");
    }
    println!(
        "    Broader history: {} sessions from this project, {} from {} other {}",
        user_breakdown.current_project_sessions,
        user_breakdown.other_project_sessions,
        user_breakdown
            .project_count
            .saturating_sub(usize::from(user_breakdown.current_project_sessions > 0)),
        if user_breakdown
            .project_count
            .saturating_sub(usize::from(user_breakdown.current_project_sessions > 0))
            == 1
        {
            "project"
        } else {
            "projects"
        }
    );
    println!();
    println!("  To enable deep analysis, set your API key:");
    println!("    harn config set api_key <your-key>");
    println!();
    println!("  Then run: harn analyze");

    Ok(())
}

fn ensure_config() -> AppResult<bool> {
    let path = HarnConfig::path();
    if path.exists() {
        return Ok(false);
    }

    let config = HarnConfig::default();
    config.save()?;
    Ok(true)
}

fn detect_stack(project_path: &Path) -> String {
    if project_path.join("package.json").exists() && project_path.join("next.config.js").exists() {
        "TypeScript / Next.js".to_string()
    } else if project_path.join("package.json").exists() {
        "JavaScript / TypeScript".to_string()
    } else if project_path.join("Cargo.toml").exists() {
        "Rust".to_string()
    } else if project_path.join("pyproject.toml").exists() {
        "Python".to_string()
    } else if project_path.join("go.mod").exists() {
        "Go".to_string()
    } else {
        "Unknown".to_string()
    }
}

fn find_claude_project_dir(project_path: &Path) -> Option<PathBuf> {
    let project_name = project_path.file_name()?.to_string_lossy().to_string();
    let candidate = dirs::home_dir()?
        .join(".claude")
        .join("projects")
        .join(project_name);
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

fn list_custom_commands(commands_dir: &Path) -> AppResult<Vec<String>> {
    if !commands_dir.exists() {
        return Ok(Vec::new());
    }

    let mut commands = Vec::new();
    for entry in fs::read_dir(commands_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
                commands.push(format!("/{stem}"));
            }
        }
    }
    commands.sort();
    Ok(commands)
}

fn install_claude_hooks(project_path: &Path) -> AppResult<()> {
    let claude_dir = project_path.join(".claude");
    fs::create_dir_all(&claude_dir)?;
    let settings_path = claude_dir.join("settings.json");

    let mut settings = if settings_path.exists() {
        serde_json::from_str::<Value>(&fs::read_to_string(&settings_path)?)
            .unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    if !settings.is_object() {
        settings = json!({});
    }

    merge_hook(&mut settings, "UserPromptSubmit", "", "harn hook prompt");
    merge_hook(
        &mut settings,
        "PostToolUse",
        "Read|Edit|Write|MultiEdit|Bash|Glob|Grep|ListDir",
        "harn hook tool",
    );
    merge_hook(&mut settings, "Stop", "", "harn hook stop");
    merge_hook(&mut settings, "SessionEnd", "", "harn hook session-end");
    merge_hook(&mut settings, "SubagentStop", "", "harn hook subagent-stop");
    merge_hook(&mut settings, "PostCompact", "", "harn hook post-compact");
    merge_hook(&mut settings, "TaskCompleted", "", "harn hook task-completed");

    fs::write(settings_path, serde_json::to_string_pretty(&settings)?)?;
    Ok(())
}

fn merge_hook(settings: &mut Value, event: &str, matcher: &str, command: &str) {
    let Value::Object(root) = settings else {
        return;
    };

    if !root.contains_key("hooks") {
        root.insert("hooks".to_string(), Value::Object(Map::new()));
    }

    let Some(Value::Object(hooks)) = root.get_mut("hooks") else {
        return;
    };

    let event_value = hooks
        .entry(event.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));

    let desired = json!({
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": command
        }]
    });

    let Some(array) = event_value.as_array_mut() else {
        *event_value = Value::Array(vec![desired]);
        return;
    };

    let exists = array.iter().any(|existing| {
        existing
            .get("hooks")
            .and_then(Value::as_array)
            .map(|hooks| {
                hooks.iter().any(|hook| {
                    hook.get("type").and_then(Value::as_str) == Some("command")
                        && hook.get("command").and_then(Value::as_str) == Some(command)
                })
            })
            .unwrap_or(false)
    });

    if !exists {
        array.push(desired);
    }
}

fn install_git_hook(project_path: &Path) -> AppResult<()> {
    let hooks_dir = project_path.join(".git").join("hooks");
    fs::create_dir_all(&hooks_dir)?;
    let hook_path = hooks_dir.join("post-commit");
    let command_line =
        r#"harn hook commit "$(git rev-parse HEAD)" "$(git branch --show-current)" "$(pwd)" &"#;

    let mut contents = if hook_path.exists() {
        fs::read_to_string(&hook_path)?
    } else {
        "#!/bin/sh\n".to_string()
    };

    if !contents.starts_with("#!") {
        contents = format!("#!/bin/sh\n{contents}");
    }

    if !contents.contains("harn hook commit") {
        if !contents.ends_with('\n') {
            contents.push('\n');
        }
        contents.push_str(command_line);
        contents.push('\n');
    }

    fs::write(&hook_path, contents)?;
    let mut permissions = fs::metadata(&hook_path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(hook_path, permissions)?;
    Ok(())
}
