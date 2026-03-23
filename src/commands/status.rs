use crate::analysis::prompt_features;
use crate::db::{self, queries};
use crate::display;
use crate::scope::AnalysisScope;
use crate::{boxed_error, AppResult};
use chrono::{Duration, Utc};
use std::collections::{HashMap, HashSet};

pub fn run(scope: AnalysisScope) -> AppResult<()> {
    let project_path = display::current_project_path()?;
    let project_string = project_path.to_string_lossy().to_string();
    let since = (Utc::now() - Duration::days(30)).to_rfc3339();
    let conn = db::connect()?;
    println!("  harn v0.1.0 -- watching your Claude Code sessions for 30 days");
    println!();

    match scope {
        AnalysisScope::Project => {
            render_scope_status(
                &conn,
                &project_string,
                &since,
                AnalysisScope::Project,
                "This Project",
            )?;
        }
        AnalysisScope::User => {
            render_scope_status(
                &conn,
                &project_string,
                &since,
                AnalysisScope::User,
                "Your Broader Claude History",
            )?;
        }
        AnalysisScope::Both => {
            render_scope_status(
                &conn,
                &project_string,
                &since,
                AnalysisScope::Project,
                "This Project",
            )?;
            println!();
            render_scope_status(
                &conn,
                &project_string,
                &since,
                AnalysisScope::User,
                "Your Broader Claude History",
            )?;
        }
    }

    println!();
    println!(
        "  Run `harn analyze --scope {}` to get specific fixes.",
        scope.as_str()
    );
    Ok(())
}

struct AreaStat {
    area: String,
    sessions: usize,
    abandoned: usize,
    explanation: String,
}

struct PromptStats {
    best_turns: i64,
    worst_turns: i64,
    specific_count: usize,
    vague_count: usize,
    specific_avg_turns: f64,
    vague_avg_turns: f64,
}

struct ErrorStats {
    common_errors: Vec<(String, usize)>,
    bash_total: usize,
    bash_failed: usize,
}

struct ThrashingStats {
    pct_sessions: f64,
    file_hotspots: Vec<(String, i64, usize)>, // (path, cycles, session_count)
    bash_hotspots: Vec<(String, i64, usize)>,  // (command, retries, session_count)
}

struct AutonomyStats {
    total_prompts: usize,
    corrections: usize,
    correction_rate: f64,
    autonomy_score: f64,
    sessions_with_corrections: usize,
    total_sessions_with_turns: usize,
    avg_corrections_per_session: f64,
}

fn render_scope_status(
    conn: &rusqlite::Connection,
    project_string: &str,
    since: &str,
    scope: AnalysisScope,
    label: &str,
) -> AppResult<()> {
    let summary = queries::load_summary_for_scope(conn, project_string, since, scope)?;
    if summary.total_sessions == 0 {
        return Err(boxed_error(format!(
            "No sessions captured for {label}. Run `harn init` and use Claude Code first."
        )));
    }

    let sessions = queries::load_sessions_for_scope(conn, project_string, since, scope)?;
    let tool_calls = queries::load_tool_calls_for_scope(conn, project_string, since, scope)?;
    let area_stats = build_area_stats(&sessions, &tool_calls);
    let prompt_stats = build_prompt_stats(&sessions);
    let error_stats = build_error_stats(&tool_calls);
    let thrashing_records =
        queries::load_thrashing_for_scope(conn, project_string, since, scope).unwrap_or_default();
    let prompt_turns =
        queries::load_prompt_turns_for_scope(conn, project_string, since, scope).unwrap_or_default();
    let thrashing_stats = build_thrashing_stats(summary.total_sessions, &thrashing_records);
    let autonomy_stats = build_autonomy_stats(&prompt_turns);

    println!("  --- {label} ---");
    println!();
    println!(
        "  {} {} captured",
        summary.total_sessions,
        if summary.total_sessions == 1 {
            "session"
        } else {
            "sessions"
        }
    );
    println!();
    println!(
        "  [ok] {} committed -- the agent helped you ship code",
        summary.committed
    );
    println!(
        "  [x] {} abandoned -- had edits but no commit",
        summary.abandoned
    );
    if summary.failed > 0 {
        println!(
            "  [!] {} failed -- errors or thrashing prevented progress",
            summary.failed
        );
    }
    if summary.exploratory > 0 {
        println!(
            "  [~] {} exploratory -- read-only sessions (no edits attempted)",
            summary.exploratory
        );
    }
    println!("      {} still in progress", summary.in_progress);
    println!();

    let actionable = summary.committed + summary.abandoned + summary.failed;
    if actionable > 0 {
        let failure_count = summary.abandoned + summary.failed;
        let failure_rate = failure_count as f64 / actionable as f64;
        println!(
            "  Your agent fails {}% of actionable sessions ({} abandoned + {} failed).",
            display::percentage(failure_count, actionable),
            summary.abandoned, summary.failed
        );
        if failure_rate >= 1.0 {
            println!("  That's every actionable session wasted.");
        } else if failure_rate <= 0.0 {
            println!("  Very little time is being wasted.");
        } else {
            println!(
                "  That's {} actionable sessions wasted.",
                display::ratio_phrase(failure_rate)
            );
        }
    } else {
        println!("  No actionable sessions yet (no edits attempted).");
    }
    println!();

    println!("  Where it struggles:");
    if area_stats.is_empty() {
        println!("  No repeat trouble spots yet -- keep capturing more sessions.");
    } else {
        for area in area_stats.iter().take(2) {
            println!(
                "  {}  {} out of {} sessions abandoned",
                area.area, area.abandoned, area.sessions
            );
            println!("           {}", area.explanation);
        }
    }
    println!();

    println!("  What goes wrong repeatedly:");
    if error_stats.common_errors.is_empty() {
        println!("  No repeated failures stand out yet.");
    } else {
        for (message, count) in error_stats.common_errors.iter().take(2) {
            println!("  \"{}\" -- hit this {} times", message, count);
        }
    }
    if error_stats.bash_total > 0 {
        println!(
            "  Bash commands fail {}% of the time.",
            display::percentage(error_stats.bash_failed, error_stats.bash_total)
        );
    }
    println!();

    println!("  How conversations go:");
    println!(
        "  Your average session takes {:.1} back-and-forth exchanges.",
        summary.avg_turns
    );
    println!(
        "  Your best sessions take {} exchange{}.",
        prompt_stats.best_turns,
        if prompt_stats.best_turns == 1 {
            ""
        } else {
            "s"
        }
    );
    println!(
        "  Your worst sessions take {}+ exchanges.",
        prompt_stats.worst_turns
    );
    if prompt_stats.specific_count > 0 && prompt_stats.vague_count > 0 {
        println!(
            "  Sessions with specific prompts average {:.1} exchanges.",
            prompt_stats.specific_avg_turns
        );
        println!(
            "  Vague prompts average {:.1} exchanges.",
            prompt_stats.vague_avg_turns
        );
    }
    println!();

    // Autonomy section
    if autonomy_stats.total_prompts > 0 {
        println!("  How autonomous your agent is:");
        if autonomy_stats.corrections == 0 {
            println!("  No corrections detected yet -- keep capturing more sessions.");
        } else {
            println!(
                "  Your agent needed human correction in {} of {} sessions.",
                autonomy_stats.sessions_with_corrections,
                autonomy_stats.total_sessions_with_turns
            );
            println!(
                "  On average, {:.1} corrections per corrected session.",
                autonomy_stats.avg_corrections_per_session
            );
            println!(
                "  Autonomy score: {:.0}% (higher is better -- fewer corrections needed).",
                autonomy_stats.autonomy_score * 100.0
            );
        }
        println!();
    }

    // Thrashing section
    if !thrashing_records.is_empty() {
        println!("  Where the agent thrashes:");
        for (path, cycles, sessions) in thrashing_stats.file_hotspots.iter().take(2) {
            println!(
                "  {} was read-then-edited {}+ times across {} session{}.",
                path,
                cycles,
                sessions,
                if *sessions == 1 { "" } else { "s" }
            );
        }
        for (cmd, retries, sessions) in thrashing_stats.bash_hotspots.iter().take(2) {
            println!(
                "  `{}` was retried {} times across {} session{}.",
                cmd,
                retries,
                sessions,
                if *sessions == 1 { "" } else { "s" }
            );
        }
        println!(
            "  {:.0}% of sessions had at least one thrashing loop.",
            thrashing_stats.pct_sessions * 100.0
        );
        println!();
    }

    println!(
        "  Roughly ${:.2} in the last 30 days across {} sessions.",
        display::approximate_cost(summary.total_tokens_in, summary.total_tokens_out),
        summary.total_sessions
    );
    if let Some(acceptance_rate) = summary.acceptance_rate {
        println!(
            "  Acceptance rate is {:.0}% on sessions where harn could measure it.",
            acceptance_rate * 100.0
        );
    } else {
        println!(
            "  Acceptance rate: not available yet -- it will show up after commits with hooks active."
        );
    }
    Ok(())
}

fn build_area_stats(
    sessions: &[queries::SessionRecord],
    tool_calls: &[queries::ToolCallRecord],
) -> Vec<AreaStat> {
    let mut per_session = HashMap::<String, HashSet<String>>::new();
    for tool_call in tool_calls {
        if let Some(file_path) = tool_call.file_path.as_deref() {
            if let Some(area) = display::top_codebase_area(file_path) {
                per_session
                    .entry(tool_call.session_id.clone())
                    .or_default()
                    .insert(area);
            }
        }
    }

    let mut aggregate = HashMap::<String, (usize, usize)>::new();
    for session in sessions {
        if let Some(areas) = per_session.get(&session.session_id) {
            for area in areas {
                let entry = aggregate.entry(area.clone()).or_insert((0, 0));
                entry.0 += 1;
                entry.1 += usize::from(session.outcome == "abandoned");
            }
        }
    }

    let mut stats = aggregate
        .into_iter()
        .map(|(area, (sessions, abandoned))| {
            let explanation = if area.contains("auth") {
                "Your agent does not know your auth patterns yet."
            } else if area.contains("db") {
                "Database work still needs clearer examples and constraints."
            } else {
                "This part of the codebase likely needs more project-specific rules."
            };
            AreaStat {
                area,
                sessions,
                abandoned,
                explanation: explanation.to_string(),
            }
        })
        .collect::<Vec<_>>();

    stats.sort_by(|left, right| right.abandoned.cmp(&left.abandoned));
    stats
}

fn build_prompt_stats(sessions: &[queries::SessionRecord]) -> PromptStats {
    let mut best_turns = i64::MAX;
    let mut worst_turns = 0_i64;
    let mut specific_count = 0_usize;
    let mut vague_count = 0_usize;
    let mut specific_turns_total = 0_i64;
    let mut vague_turns_total = 0_i64;

    for session in sessions {
        best_turns = best_turns.min(session.total_turns);
        worst_turns = worst_turns.max(session.total_turns);
        let features = prompt_features::analyze(session.prompt_text.as_deref().unwrap_or(""));
        if features.specific {
            specific_count += 1;
            specific_turns_total += session.total_turns;
        } else {
            vague_count += 1;
            vague_turns_total += session.total_turns;
        }
    }

    PromptStats {
        best_turns: if best_turns == i64::MAX {
            0
        } else {
            best_turns
        },
        worst_turns,
        specific_count,
        vague_count,
        specific_avg_turns: if specific_count == 0 {
            0.0
        } else {
            specific_turns_total as f64 / specific_count as f64
        },
        vague_avg_turns: if vague_count == 0 {
            0.0
        } else {
            vague_turns_total as f64 / vague_count as f64
        },
    }
}

fn build_error_stats(tool_calls: &[queries::ToolCallRecord]) -> ErrorStats {
    let mut errors = HashMap::<String, usize>::new();
    let mut bash_total = 0_usize;
    let mut bash_failed = 0_usize;

    for tool_call in tool_calls {
        if tool_call.tool_name == "Bash" {
            bash_total += 1;
            if !tool_call.success {
                bash_failed += 1;
            }
        }
        if !tool_call.success {
            let message = tool_call
                .error_text
                .as_deref()
                .unwrap_or("tool failed")
                .lines()
                .next()
                .unwrap_or("tool failed")
                .trim()
                .to_string();
            *errors.entry(message).or_insert(0) += 1;
        }
    }

    let mut common_errors = errors.into_iter().collect::<Vec<_>>();
    common_errors.sort_by(|left, right| right.1.cmp(&left.1));

    ErrorStats {
        common_errors,
        bash_total,
        bash_failed,
    }
}

fn build_thrashing_stats(
    total_sessions: usize,
    thrashing: &[queries::ThrashingRecord],
) -> ThrashingStats {
    let session_ids: HashSet<&str> = thrashing.iter().map(|r| r.session_id.as_str()).collect();
    let pct_sessions = if total_sessions == 0 {
        0.0
    } else {
        session_ids.len() as f64 / total_sessions as f64
    };

    let mut file_map: HashMap<&str, (i64, HashSet<&str>)> = HashMap::new();
    let mut bash_map: HashMap<&str, (i64, HashSet<&str>)> = HashMap::new();

    for record in thrashing {
        let map = if record.thrash_type == "file_cycle" {
            &mut file_map
        } else {
            &mut bash_map
        };
        let entry = map
            .entry(record.target.as_str())
            .or_insert_with(|| (0, HashSet::new()));
        entry.0 += record.cycle_count;
        entry.1.insert(record.session_id.as_str());
    }

    let mut file_hotspots: Vec<_> = file_map
        .into_iter()
        .map(|(path, (cycles, sessions))| (path.to_string(), cycles, sessions.len()))
        .collect();
    file_hotspots.sort_by(|a, b| b.1.cmp(&a.1));

    let mut bash_hotspots: Vec<_> = bash_map
        .into_iter()
        .map(|(cmd, (retries, sessions))| (cmd.to_string(), retries, sessions.len()))
        .collect();
    bash_hotspots.sort_by(|a, b| b.1.cmp(&a.1));

    ThrashingStats {
        pct_sessions,
        file_hotspots,
        bash_hotspots,
    }
}

fn build_autonomy_stats(prompt_turns: &[queries::PromptTurnRecord]) -> AutonomyStats {
    let mut corrections = 0_usize;
    let mut follow_ups = 0_usize;
    let mut sessions_with_corrections: HashSet<&str> = HashSet::new();
    let all_sessions: HashSet<&str> = prompt_turns.iter().map(|t| t.session_id.as_str()).collect();

    for turn in prompt_turns {
        match turn.classification.as_str() {
            "correction" => {
                corrections += 1;
                sessions_with_corrections.insert(&turn.session_id);
            }
            "follow_up" => follow_ups += 1,
            _ => {}
        }
    }

    let non_initial = corrections + follow_ups;
    let correction_rate = if non_initial == 0 {
        0.0
    } else {
        corrections as f64 / non_initial as f64
    };

    AutonomyStats {
        total_prompts: prompt_turns.len(),
        corrections,
        correction_rate,
        autonomy_score: 1.0 - correction_rate,
        sessions_with_corrections: sessions_with_corrections.len(),
        total_sessions_with_turns: all_sessions.len(),
        avg_corrections_per_session: if sessions_with_corrections.is_empty() {
            0.0
        } else {
            corrections as f64 / sessions_with_corrections.len() as f64
        },
    }
}
