use crate::analysis::stage2::GeneratedConfig;
use crate::capture::session_end;
use crate::db::{self, queries};
use crate::display;
use crate::{boxed_error, AppResult};
use std::fs;
use std::io::{self, Write};
use std::process::Command;

pub fn run() -> AppResult<()> {
    let conn = db::connect()?;
    let Some(run) = queries::latest_analysis_run(&conn)? else {
        return Err(boxed_error(
            "No analysis run found yet. Run `harn analyze` first.",
        ));
    };

    let generated_configs: Vec<GeneratedConfig> =
        serde_json::from_str(&run.generated_configs_json).unwrap_or_default();

    let agents_configs: Vec<_> = generated_configs
        .iter()
        .filter(|c| c.target == "agents_md")
        .collect();
    let claude_configs: Vec<_> = generated_configs
        .iter()
        .filter(|c| c.target == "claude_md")
        .collect();
    let command_configs: Vec<_> = generated_configs
        .iter()
        .filter(|c| c.target == "custom_command")
        .collect();
    let workflow_configs: Vec<_> = generated_configs
        .iter()
        .filter(|c| c.target == "user_workflow")
        .collect();

    let has_applicable = !agents_configs.is_empty()
        || !claude_configs.is_empty()
        || !command_configs.is_empty();

    if !has_applicable && workflow_configs.is_empty() {
        return Err(boxed_error(
            "The latest analysis did not include any changes. Run `harn analyze` first.",
        ));
    }

    let project_path = display::current_project_path()?;

    if !agents_configs.is_empty() {
        println!();
        println!("  --- AGENTS.md Changes ---");
        println!();
        apply_file_configs(
            &project_path.join("AGENTS.md"),
            "AGENTS.md",
            &agents_configs,
        )?;
    }

    if !claude_configs.is_empty() {
        println!();
        println!("  --- CLAUDE.md Changes ---");
        println!();
        apply_file_configs(
            &project_path.join("CLAUDE.md"),
            "CLAUDE.md",
            &claude_configs,
        )?;
    }

    if !command_configs.is_empty() {
        println!();
        println!("  --- Custom Commands ---");
        println!();
        apply_command_configs(&project_path, &command_configs)?;
    }

    if !workflow_configs.is_empty() {
        println!();
        println!("  --- Workflow Recommendations ---");
        println!();
        for (index, config) in workflow_configs.iter().enumerate() {
            println!("  {}. {}", index + 1, config.title);
            if !config.reason.trim().is_empty() {
                println!("     Why: {}", config.reason.trim());
            }
            if !config.new_text.trim().is_empty() {
                println!();
                for line in config.new_text.trim().lines() {
                    println!("     {}", line);
                }
            }
            if !config.expected_impact.trim().is_empty() {
                println!();
                println!("     Expected impact: {}", config.expected_impact.trim());
            }
            println!();
        }
    }

    let conn = db::connect()?;
    session_end::snapshot_harness_files(&conn, &project_path, None)?;

    Ok(())
}

fn apply_file_configs(
    file_path: &std::path::Path,
    label: &str,
    configs: &[&GeneratedConfig],
) -> AppResult<()> {
    let current_content = fs::read_to_string(file_path).unwrap_or_default();
    let (mut proposed_content, changes) = apply_configs(&current_content, configs);
    if proposed_content.trim().is_empty() {
        proposed_content = current_content.clone();
    }

    println!(
        "  Current {}: {} rules, {} tokens",
        label,
        display::count_rules(&current_content),
        display::token_count(&current_content)
    );
    println!(
        "  After changes:  {} rules, {} tokens ({:+})",
        display::count_rules(&proposed_content),
        display::token_count(&proposed_content),
        display::token_count(&proposed_content) as isize
            - display::token_count(&current_content) as isize
    );
    println!();

    for (index, change) in changes.iter().enumerate() {
        println!(
            "  Change {} of {}: {}",
            index + 1,
            changes.len(),
            change.title
        );
        if let Some(removed) = change.removed.as_deref() {
            if !removed.trim().is_empty() {
                println!();
                println!("  REMOVE:");
                println!("{}", display::format_box(removed.trim()));
            }
        }
        if let Some(added) = change.added.as_deref() {
            if !added.trim().is_empty() {
                println!();
                println!("  ADD:");
                println!("{}", display::format_box(added.trim()));
            }
        }
        if !change.reason.trim().is_empty() {
            println!();
            println!("  Reason: {}", change.reason.trim());
        }
        println!();
    }

    loop {
        println!("  Apply these changes to {}?", label);
        println!("  [a] Apply");
        println!("  [s] Skip");
        println!("  [e] Edit first");
        println!("  [d] Show full diff");
        print!("  > ");
        io::stdout().flush()?;

        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;
        match choice.trim().to_lowercase().as_str() {
            "a" => {
                let backup_path =
                    file_path.with_file_name(format!("{}.harn-backup", label));
                if file_path.exists() {
                    fs::write(&backup_path, &current_content)?;
                    println!("  [ok] Backed up to {}", backup_path.display());
                }
                fs::write(file_path, &proposed_content)?;
                println!("  [ok] Applied {} changes to {}", changes.len(), label);
                break;
            }
            "s" => {
                println!("  Skipped {}.", label);
                break;
            }
            "d" => {
                println!();
                println!(
                    "{}",
                    display::render_unified_diff(&current_content, &proposed_content)
                );
                println!();
            }
            "e" => {
                proposed_content = open_in_editor(&proposed_content)?;
                println!("  Updated proposed content from your editor.");
                println!();
            }
            _ => {
                println!("  Choose `a`, `s`, `e`, or `d`.");
            }
        }
    }

    Ok(())
}

fn apply_command_configs(
    project_path: &std::path::Path,
    configs: &[&GeneratedConfig],
) -> AppResult<()> {
    let commands_dir = project_path.join(".claude").join("commands");
    fs::create_dir_all(&commands_dir)?;

    for config in configs {
        let name = config
            .title
            .trim()
            .trim_start_matches('/')
            .replace(' ', "-")
            .to_lowercase();
        let command_path = commands_dir.join(format!("{name}.md"));
        let new_text = config.new_text.trim();

        if new_text.is_empty() {
            continue;
        }

        println!("  Command: /{name}");
        if !config.reason.trim().is_empty() {
            println!("  Reason: {}", config.reason.trim());
        }
        println!();
        for line in new_text.lines() {
            println!("    {line}");
        }
        println!();

        if command_path.exists() {
            println!("  /{name} already exists, skipping.");
            continue;
        }

        print!("  Create /{name}? [y/n] > ");
        io::stdout().flush()?;
        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;
        if choice.trim().to_lowercase() == "y" {
            fs::write(&command_path, new_text)?;
            println!("  [ok] Created {}", command_path.display());
        } else {
            println!("  Skipped.");
        }
        println!();
    }

    Ok(())
}

struct PlannedChange {
    title: String,
    removed: Option<String>,
    added: Option<String>,
    reason: String,
}

fn apply_configs(
    current_content: &str,
    configs: &[&GeneratedConfig],
) -> (String, Vec<PlannedChange>) {
    let mut content = current_content.to_string();
    let mut changes = Vec::new();

    for config in configs {
        let action = config.action.to_lowercase();
        let target = config.target_text.trim();
        let new_text = config.new_text.trim();
        let mut removed = None;
        let mut added = None;

        match action.as_str() {
            "remove" => {
                if !target.is_empty() && content.contains(target) {
                    content = content.replacen(target, "", 1);
                    removed = Some(target.to_string());
                }
            }
            "rewrite" => {
                if !target.is_empty() && content.contains(target) {
                    content = content.replacen(target, new_text, 1);
                    removed = Some(target.to_string());
                    if !new_text.is_empty() {
                        added = Some(new_text.to_string());
                    }
                } else if !new_text.is_empty() {
                    if !content.trim().is_empty() {
                        content.push_str("\n\n");
                    }
                    content.push_str(new_text);
                    added = Some(new_text.to_string());
                }
            }
            _ => {
                if !new_text.is_empty() {
                    if !content.trim().is_empty() {
                        content.push_str("\n\n");
                    }
                    content.push_str(new_text);
                    added = Some(new_text.to_string());
                }
            }
        }

        changes.push(PlannedChange {
            title: config.title.clone(),
            removed,
            added,
            reason: config.reason.clone(),
        });
    }

    while content.contains("\n\n\n") {
        content = content.replace("\n\n\n", "\n\n");
    }

    (content.trim().to_string() + "\n", changes)
}

fn open_in_editor(initial_content: &str) -> AppResult<String> {
    let temp_path = std::env::temp_dir().join(format!(
        "harn-agents-{}.md",
        chrono::Utc::now().timestamp_millis()
    ));
    fs::write(&temp_path, initial_content)?;

    let editor = std::env::var("EDITOR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "vim".to_string());
    let status = Command::new(&editor).arg(&temp_path).status();
    if status
        .as_ref()
        .map(|status| !status.success())
        .unwrap_or(true)
    {
        let fallback = Command::new("nano").arg(&temp_path).status()?;
        if !fallback.success() {
            return Err(boxed_error("unable to open an editor"));
        }
    }

    Ok(fs::read_to_string(temp_path)?)
}
