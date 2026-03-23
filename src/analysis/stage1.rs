use crate::analysis::prompt_features;
use crate::db::{self, queries};
use crate::display;
use crate::scope::AnalysisScope;
use crate::AppResult;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

pub struct Stage1Bundle {
    pub payload: Value,
    pub session_count: usize,
    pub period_label: String,
    pub project_session_count: usize,
    pub user_session_count: usize,
}

pub fn build(project_path: &Path, days: i64, scope: AnalysisScope) -> AppResult<Stage1Bundle> {
    let conn = db::connect()?;
    let project_string = project_path.to_string_lossy().to_string();
    let since = (Utc::now() - Duration::days(days)).to_rfc3339();

    let project_sessions =
        queries::load_sessions_for_scope(&conn, &project_string, &since, AnalysisScope::Project)?;
    let project_tool_calls =
        queries::load_tool_calls_for_scope(&conn, &project_string, &since, AnalysisScope::Project)?;
    let project_thrashing =
        queries::load_thrashing_for_scope(&conn, &project_string, &since, AnalysisScope::Project)?;
    let project_prompt_turns =
        queries::load_prompt_turns_for_scope(&conn, &project_string, &since, AnalysisScope::Project)?;
    let project_summary = queries::summarize_sessions(
        &project_sessions,
        queries::load_acceptance_rate_for_scope(
            &conn,
            &project_string,
            &since,
            AnalysisScope::Project,
        )?,
    );

    let user_sessions =
        queries::load_sessions_for_scope(&conn, &project_string, &since, AnalysisScope::User)?;
    let user_tool_calls =
        queries::load_tool_calls_for_scope(&conn, &project_string, &since, AnalysisScope::User)?;
    let user_thrashing =
        queries::load_thrashing_for_scope(&conn, &project_string, &since, AnalysisScope::User)?;
    let user_prompt_turns =
        queries::load_prompt_turns_for_scope(&conn, &project_string, &since, AnalysisScope::User)?;
    let user_summary = queries::summarize_sessions(
        &user_sessions,
        queries::load_acceptance_rate_for_scope(
            &conn,
            &project_string,
            &since,
            AnalysisScope::User,
        )?,
    );

    let agents_content = fs::read_to_string(project_path.join("AGENTS.md")).ok();
    let custom_commands = list_custom_commands(project_path)?;
    let scope_payload = match scope {
        AnalysisScope::Project => {
            json!({
                "project": build_scope_slice(
                    "project",
                    &project_string,
                    &project_summary,
                    &project_sessions,
                    &project_tool_calls,
                    agents_content.as_deref(),
                    &custom_commands,
                    &project_thrashing,
                    &project_prompt_turns,
                )
            })
        }
        AnalysisScope::User => {
            json!({
                "user": build_scope_slice(
                    "user",
                    &project_string,
                    &user_summary,
                    &user_sessions,
                    &user_tool_calls,
                    None,
                    &[],
                    &user_thrashing,
                    &user_prompt_turns,
                )
            })
        }
        AnalysisScope::Both => {
            json!({
                "project": build_scope_slice(
                    "project",
                    &project_string,
                    &project_summary,
                    &project_sessions,
                    &project_tool_calls,
                    agents_content.as_deref(),
                    &custom_commands,
                    &project_thrashing,
                    &project_prompt_turns,
                ),
                "user": build_scope_slice(
                    "user",
                    &project_string,
                    &user_summary,
                    &user_sessions,
                    &user_tool_calls,
                    None,
                    &[],
                    &user_thrashing,
                    &user_prompt_turns,
                ),
            })
        }
    };

    let payload = json!({
        "analysis_scope": scope.as_str(),
        "current_project_path": project_string,
        "scope_description": scope.headline(),
        "scopes": scope_payload,
        "comparison": {
            "current_project_sessions": project_summary.total_sessions,
            "broader_user_sessions": user_summary.total_sessions,
            "project_share_of_user_history": if user_summary.total_sessions == 0 {
                0.0
            } else {
                project_summary.total_sessions as f64 / user_summary.total_sessions as f64
            }
        }
    });

    let session_count = match scope {
        AnalysisScope::Project => project_summary.total_sessions,
        AnalysisScope::User | AnalysisScope::Both => user_summary.total_sessions,
    };
    let period_label = match scope {
        AnalysisScope::Project => display::human_period(
            project_summary.min_started_at.as_deref(),
            project_summary.max_started_at.as_deref(),
        ),
        AnalysisScope::User | AnalysisScope::Both => display::human_period(
            user_summary.min_started_at.as_deref(),
            user_summary.max_started_at.as_deref(),
        ),
    };

    Ok(Stage1Bundle {
        payload,
        session_count,
        period_label,
        project_session_count: project_summary.total_sessions,
        user_session_count: user_summary.total_sessions,
    })
}

fn build_scope_slice(
    slice_scope: &str,
    project_path: &str,
    summary: &queries::ProjectSummary,
    sessions: &[queries::SessionRecord],
    tool_calls: &[queries::ToolCallRecord],
    agents_content: Option<&str>,
    custom_commands: &[String],
    thrashing_records: &[queries::ThrashingRecord],
    prompt_turns: &[queries::PromptTurnRecord],
) -> Value {
    let aggregate = prompt_features::aggregate(
        sessions
            .iter()
            .filter_map(|session| session.prompt_text.as_deref()),
    );
    let total_tokens = summary.total_tokens_in + summary.total_tokens_out;

    json!({
        "scope": slice_scope,
        "session_summary": {
            "period": display::human_period(
                summary.min_started_at.as_deref(),
                summary.max_started_at.as_deref()
            ),
            "total_sessions": summary.total_sessions,
            "by_outcome": {
                "committed": summary.committed,
                "abandoned": summary.abandoned,
                "failed": summary.failed,
                "exploratory": summary.exploratory,
                "in_progress": summary.in_progress
            },
            "avg_iterations": summary.avg_turns,
            "total_tokens": total_tokens,
            "compaction_events": 0
        },
        "acceptance_rate": {
            "overall": summary.acceptance_rate,
            "by_codebase_area": {}
        },
        "prompt_features_aggregate": {
            "avg_prompt_length_words": aggregate.avg_prompt_length_words,
            "pct_with_file_path": aggregate.pct_with_file_path,
            "pct_with_reference_impl": aggregate.pct_with_reference_impl,
            "pct_with_negative_constraint": aggregate.pct_with_negative_constraint,
            "pct_specific": aggregate.pct_specific
        },
        "feature_outcome_correlations": build_feature_correlations(sessions),
        "harness_files": {
            "agents_md": {
                "exists": agents_content.is_some(),
                "total_tokens": agents_content.map(display::token_count).unwrap_or(0),
                "rules": agents_content.map(extract_rules).unwrap_or_default()
            },
            "custom_commands": custom_commands
        },
        "rule_compliance": [],
        "codebase_areas": build_codebase_areas(sessions, tool_calls),
        "tool_patterns": build_tool_patterns(sessions.len(), tool_calls),
        "thrashing_patterns": build_thrashing_patterns(sessions.len(), thrashing_records),
        "autonomy_metrics": build_autonomy_metrics(prompt_turns),
        "example_sessions": {
            "best": best_sessions(sessions),
            "worst": worst_sessions(sessions)
        },
        "project_coverage": {
            "current_project_path": project_path,
            "project_sessions_in_slice": sessions.iter().filter(|session| session.project_path.as_deref() == Some(project_path)).count(),
            "distinct_projects": sessions.iter().filter_map(|session| session.project_path.as_deref()).collect::<HashSet<_>>().len()
        }
    })
}

fn build_feature_correlations(sessions: &[queries::SessionRecord]) -> Vec<Value> {
    let mut buckets = vec![
        (
            "has_file_path",
            0_usize,
            0_usize,
            0_i64,
            0_usize,
            0_usize,
            0_i64,
        ),
        (
            "has_reference_impl",
            0_usize,
            0_usize,
            0_i64,
            0_usize,
            0_usize,
            0_i64,
        ),
        (
            "has_negative_constraint",
            0_usize,
            0_usize,
            0_i64,
            0_usize,
            0_usize,
            0_i64,
        ),
    ];

    for session in sessions {
        let prompt = session.prompt_text.as_deref().unwrap_or("");
        let features = prompt_features::analyze(prompt);
        let committed = session.outcome == "committed";

        for bucket in &mut buckets {
            let present = match bucket.0 {
                "has_file_path" => features.has_file_path,
                "has_reference_impl" => features.has_reference_impl,
                _ => features.has_negative_constraint,
            };

            if present {
                bucket.1 += 1;
                bucket.2 += usize::from(committed);
                bucket.3 += session.total_turns;
            } else {
                bucket.4 += 1;
                bucket.5 += usize::from(committed);
                bucket.6 += session.total_turns;
            }
        }
    }

    buckets
        .into_iter()
        .map(|bucket| {
            json!({
                "feature": bucket.0,
                "present_commit_rate": if bucket.1 == 0 { 0.0 } else { bucket.2 as f64 / bucket.1 as f64 },
                "absent_commit_rate": if bucket.4 == 0 { 0.0 } else { bucket.5 as f64 / bucket.4 as f64 },
                "present_avg_turns": if bucket.1 == 0 { 0.0 } else { bucket.3 as f64 / bucket.1 as f64 },
                "absent_avg_turns": if bucket.4 == 0 { 0.0 } else { bucket.6 as f64 / bucket.4 as f64 }
            })
        })
        .collect()
}

fn build_codebase_areas(
    sessions: &[queries::SessionRecord],
    tool_calls: &[queries::ToolCallRecord],
) -> Vec<Value> {
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

    let mut areas = HashMap::<String, (usize, usize, i64)>::new();
    for session in sessions {
        if let Some(session_areas) = per_session.get(&session.session_id) {
            for area in session_areas {
                let entry = areas.entry(area.clone()).or_insert((0, 0, 0));
                entry.0 += 1;
                entry.1 += usize::from(session.outcome == "abandoned");
                entry.2 += session.total_turns;
            }
        }
    }

    let mut values = areas
        .into_iter()
        .map(|(area, (sessions_count, abandoned, turns_total))| {
            let avg_turns = if sessions_count == 0 {
                0.0
            } else {
                turns_total as f64 / sessions_count as f64
            };
            let explanation = if area.contains("auth") {
                "Auth work is underspecified in the current harness."
            } else if area.contains("db") {
                "Database work is taking extra iterations."
            } else {
                "This area likely needs more concrete project rules."
            };
            json!({
                "area": area,
                "sessions": sessions_count,
                "abandoned": abandoned,
                "avg_turns": avg_turns,
                "explanation": explanation
            })
        })
        .collect::<Vec<_>>();

    values.sort_by(|left, right| {
        let right_abandoned = right.get("abandoned").and_then(Value::as_u64).unwrap_or(0);
        let left_abandoned = left.get("abandoned").and_then(Value::as_u64).unwrap_or(0);
        right_abandoned.cmp(&left_abandoned)
    });
    values.truncate(5);
    values
}

fn build_tool_patterns(total_sessions: usize, tool_calls: &[queries::ToolCallRecord]) -> Value {
    let mut per_session = HashMap::<String, Vec<&queries::ToolCallRecord>>::new();
    let mut bash_total = 0_usize;
    let mut bash_failed = 0_usize;
    let mut common_bash_errors = HashMap::<String, usize>::new();

    for tool_call in tool_calls {
        per_session
            .entry(tool_call.session_id.clone())
            .or_default()
            .push(tool_call);

        if tool_call.tool_name == "Bash" {
            bash_total += 1;
            if !tool_call.success {
                bash_failed += 1;
                let error = tool_call
                    .error_text
                    .as_deref()
                    .unwrap_or("bash command failed")
                    .lines()
                    .next()
                    .unwrap_or("bash command failed")
                    .trim()
                    .to_string();
                *common_bash_errors.entry(error).or_insert(0) += 1;
            }
        }
    }

    let mut edit_without_read = 0_usize;
    let mut with_test_run = 0_usize;
    for tool_calls in per_session.values() {
        let mut saw_read = false;
        let mut saw_edit_before_read = false;
        let mut saw_test = false;

        let mut ordered = tool_calls.clone();
        ordered.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));

        for tool_call in ordered {
            match tool_call.tool_name.as_str() {
                "Read" | "Glob" | "Grep" | "ListDir" => saw_read = true,
                "Edit" | "Write" | "MultiEdit" if !saw_read => saw_edit_before_read = true,
                "Bash" => {
                    if tool_call
                        .command
                        .as_deref()
                        .map(|command| {
                            let lower = command.to_lowercase();
                            lower.contains(" test")
                                || lower.contains("cargo test")
                                || lower.contains("npm test")
                                || lower.contains("pnpm test")
                                || lower.contains("pytest")
                        })
                        .unwrap_or(false)
                    {
                        saw_test = true;
                    }
                }
                _ => {}
            }
        }

        edit_without_read += usize::from(saw_edit_before_read);
        with_test_run += usize::from(saw_test);
    }

    let mut errors = common_bash_errors.into_iter().collect::<Vec<_>>();
    errors.sort_by(|left, right| right.1.cmp(&left.1));
    errors.truncate(5);

    json!({
        "pct_sessions_edit_without_read": if total_sessions == 0 { 0.0 } else { edit_without_read as f64 / total_sessions as f64 },
        "pct_sessions_with_test_run": if total_sessions == 0 { 0.0 } else { with_test_run as f64 / total_sessions as f64 },
        "bash_failure_rate": if bash_total == 0 { 0.0 } else { bash_failed as f64 / bash_total as f64 },
        "common_bash_errors": errors.into_iter().map(|(message, count)| json!({"message": message, "count": count})).collect::<Vec<_>>()
    })
}

fn build_thrashing_patterns(
    total_sessions: usize,
    thrashing: &[queries::ThrashingRecord],
) -> Value {
    if thrashing.is_empty() {
        return json!({
            "pct_sessions_with_thrashing": 0.0,
            "total_thrashing_sessions": 0,
            "file_cycle_hotspots": [],
            "bash_retry_hotspots": [],
            "avg_cycles_per_thrashing_session": 0.0
        });
    }

    let thrashing_session_ids: HashSet<&str> = thrashing.iter().map(|r| r.session_id.as_str()).collect();
    let thrashing_session_count = thrashing_session_ids.len();

    // Aggregate file cycle hotspots
    let mut file_hotspots: HashMap<&str, (i64, HashSet<&str>)> = HashMap::new();
    let mut bash_hotspots: HashMap<&str, (i64, HashSet<&str>)> = HashMap::new();
    let mut total_cycles: i64 = 0;

    for record in thrashing {
        total_cycles += record.cycle_count;
        let map = if record.thrash_type == "file_cycle" {
            &mut file_hotspots
        } else {
            &mut bash_hotspots
        };
        let entry = map.entry(record.target.as_str()).or_insert_with(|| (0, HashSet::new()));
        entry.0 += record.cycle_count;
        entry.1.insert(record.session_id.as_str());
    }

    let mut file_list: Vec<_> = file_hotspots.into_iter().collect();
    file_list.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));

    let mut bash_list: Vec<_> = bash_hotspots.into_iter().collect();
    bash_list.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));

    json!({
        "pct_sessions_with_thrashing": if total_sessions == 0 { 0.0 } else { thrashing_session_count as f64 / total_sessions as f64 },
        "total_thrashing_sessions": thrashing_session_count,
        "file_cycle_hotspots": file_list.iter().take(5).map(|(path, (cycles, sessions))| {
            json!({"file": path, "total_cycles": cycles, "sessions": sessions.len()})
        }).collect::<Vec<_>>(),
        "bash_retry_hotspots": bash_list.iter().take(5).map(|(cmd, (count, sessions))| {
            json!({"command": cmd, "total_retries": count, "sessions": sessions.len()})
        }).collect::<Vec<_>>(),
        "avg_cycles_per_thrashing_session": if thrashing_session_count == 0 { 0.0 } else { total_cycles as f64 / thrashing_session_count as f64 }
    })
}

fn build_autonomy_metrics(prompt_turns: &[queries::PromptTurnRecord]) -> Value {
    if prompt_turns.is_empty() {
        return json!({
            "total_prompts": 0,
            "initial_prompts": 0,
            "follow_ups": 0,
            "corrections": 0,
            "correction_rate": 0.0,
            "autonomy_score": 1.0,
            "sessions_with_corrections": 0,
            "avg_corrections_per_corrected_session": 0.0,
            "correction_examples": []
        });
    }

    let mut initial = 0_usize;
    let mut follow_ups = 0_usize;
    let mut corrections = 0_usize;
    let mut sessions_with_corrections: HashSet<&str> = HashSet::new();
    let mut correction_examples: Vec<Value> = Vec::new();

    for turn in prompt_turns {
        match turn.classification.as_str() {
            "initial_prompt" => initial += 1,
            "follow_up" => follow_ups += 1,
            "correction" => {
                corrections += 1;
                sessions_with_corrections.insert(&turn.session_id);
                if correction_examples.len() < 5 {
                    correction_examples.push(json!({
                        "session_id": turn.session_id,
                        "prompt": if turn.prompt_text.len() > 120 {
                            format!("{}...", &turn.prompt_text[..120])
                        } else {
                            turn.prompt_text.clone()
                        },
                        "turn": turn.turn_number
                    }));
                }
            }
            _ => follow_ups += 1,
        }
    }

    let total = prompt_turns.len();
    // Correction rate is corrections / non-initial prompts (follow-ups + corrections)
    let non_initial = follow_ups + corrections;
    let correction_rate = if non_initial == 0 {
        0.0
    } else {
        corrections as f64 / non_initial as f64
    };

    json!({
        "total_prompts": total,
        "initial_prompts": initial,
        "follow_ups": follow_ups,
        "corrections": corrections,
        "correction_rate": correction_rate,
        "autonomy_score": 1.0 - correction_rate,
        "sessions_with_corrections": sessions_with_corrections.len(),
        "avg_corrections_per_corrected_session": if sessions_with_corrections.is_empty() { 0.0 } else { corrections as f64 / sessions_with_corrections.len() as f64 },
        "correction_examples": correction_examples
    })
}

fn best_sessions(sessions: &[queries::SessionRecord]) -> Vec<Value> {
    let mut candidates = sessions
        .iter()
        .filter(|session| session.outcome == "committed")
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.total_turns.cmp(&right.total_turns));
    candidates
        .into_iter()
        .take(3)
        .map(|session| {
            json!({
                "session_id": session.session_id,
                "prompt": session.prompt_text,
                "turns": session.total_turns,
                "outcome": session.outcome
            })
        })
        .collect()
}

fn worst_sessions(sessions: &[queries::SessionRecord]) -> Vec<Value> {
    let mut candidates = sessions.iter().collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.total_turns.cmp(&left.total_turns));
    candidates
        .into_iter()
        .take(3)
        .map(|session| {
            json!({
                "session_id": session.session_id,
                "prompt": session.prompt_text,
                "turns": session.total_turns,
                "outcome": session.outcome
            })
        })
        .collect()
}

fn list_custom_commands(project_path: &Path) -> AppResult<Vec<String>> {
    let commands_dir = project_path.join(".claude").join("commands");
    if !commands_dir.exists() {
        return Ok(Vec::new());
    }

    let mut commands = Vec::new();
    for entry in fs::read_dir(commands_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            commands.push(path.file_name().unwrap().to_string_lossy().to_string());
        }
    }
    commands.sort();
    Ok(commands)
}

fn extract_rules(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.starts_with("- ")
                || line.starts_with("* ")
                || line.starts_with("## ")
                || line.starts_with("### ")
        })
        .take(20)
        .map(ToOwned::to_owned)
        .collect()
}
