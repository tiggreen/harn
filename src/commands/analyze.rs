use crate::analysis::{stage1, stage2};
use crate::config::HarnConfig;
use crate::db::{self, queries};
use crate::display;
use crate::scope::AnalysisScope;
use crate::{boxed_error, AppResult};
use crossterm::style::{Attribute, Color, Stylize};

pub fn run(scope: AnalysisScope) -> AppResult<()> {
    let config = HarnConfig::load();
    let Some((provider, api_key, model)) = config.resolve_provider() else {
        return Err(boxed_error(
            "No API key found. Set one with:\n  harn config set api_key <anthropic-key>\n  harn config set openai_api_key <openai-key>\nOr set ANTHROPIC_API_KEY or OPENAI_API_KEY env vars.",
        ));
    };

    let project_path = display::current_project_path()?;
    let bundle = stage1::build(&project_path, 30, scope)?;
    if bundle.session_count == 0 {
        return Err(boxed_error(
            "No sessions found for this project yet. Run `harn init` and use Claude Code first.",
        ));
    }

    let provider_name = match provider {
        crate::config::LlmProvider::Anthropic => "Anthropic",
        crate::config::LlmProvider::OpenAI => "OpenAI",
    };

    println!();
    print_header("harn analyze");

    let scope_msg = match scope {
        AnalysisScope::Project => format!(
            "{} project sessions from {}",
            bundle.project_session_count, bundle.period_label
        ),
        AnalysisScope::User => format!(
            "{} sessions from broader history ({})",
            bundle.user_session_count, bundle.period_label
        ),
        AnalysisScope::Both => format!(
            "{} sessions ({} from this project) | {}",
            bundle.user_session_count, bundle.project_session_count, bundle.period_label
        ),
    };
    println!(
        "  {} {}",
        "Scope:".with(Color::DarkGrey),
        scope_msg
    );
    println!(
        "  {} {} via {}",
        "Model:".with(Color::DarkGrey),
        model.clone().with(Color::Cyan),
        provider_name.with(Color::Cyan)
    );
    println!();

    let report = stage2::analyze(provider, &api_key, &model, &bundle.payload)?;
    let conn = db::connect()?;
    queries::insert_analysis_run(
        &conn,
        &display::now_rfc3339(),
        bundle.session_count as i64,
        report.harness_score,
        &serde_json::to_string(&report.findings)?,
        &serde_json::to_string(&report.generated_configs)?,
    )?;

    print_score(report.harness_score);

    if !report.summary.trim().is_empty() {
        println!();
        print_box("Summary", report.summary.trim(), Color::Blue);
    }

    if report.findings.is_empty() {
        println!();
        println!(
            "  {} No clear problems stood out yet.",
            "~".with(Color::Green)
        );
        println!(
            "  {} Keep capturing sessions and run {} again in a few days.",
            " ".with(Color::Reset),
            "harn analyze".with(Color::Cyan)
        );
        println!();
        return Ok(());
    }

    let project_findings: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.scope == "project")
        .collect();
    let user_findings: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.scope == "user")
        .collect();
    let project_changes: Vec<_> = report
        .generated_configs
        .iter()
        .filter(|c| c.scope == "project" && c.target == "agents_md")
        .collect();

    if !project_findings.is_empty() {
        println!();
        print_section_header("Project Harness Fixes");
        print_findings(&project_findings, &report.generated_configs);
    }

    if !user_findings.is_empty() {
        println!();
        print_section_header("Your Workflow Improvements");
        print_findings(&user_findings, &report.generated_configs);
    }

    println!();
    print_divider();
    if project_changes.is_empty() {
        println!(
            "  {} No project-scoped AGENTS.md changes are ready yet.",
            "~".with(Color::DarkYellow)
        );
    } else {
        println!(
            "  {} {} ready to apply. Run {}",
            "+".with(Color::Green),
            format!("{} fix{}", project_changes.len(), if project_changes.len() == 1 { "" } else { "es" }),
            "harn generate".with(Color::Cyan).attribute(Attribute::Bold)
        );
    }
    if !user_findings.is_empty() {
        println!(
            "  {} User-level improvements are guidance, not automatic rewrites.",
            "i".with(Color::Blue)
        );
    }
    println!();

    Ok(())
}

fn print_header(title: &str) {
    let line = "─".repeat(50);
    println!("  {}", line.clone().with(Color::DarkGrey));
    println!(
        "  {}  {}",
        title.with(Color::White).attribute(Attribute::Bold),
        "v0.1.0".with(Color::DarkGrey)
    );
    println!("  {}", line.with(Color::DarkGrey));
}

fn print_divider() {
    println!("  {}", "─".repeat(50).with(Color::DarkGrey));
}

fn print_score(score: i64) {
    let (color, label) = match score {
        0..=30 => (Color::Red, "Needs Work"),
        31..=60 => (Color::DarkYellow, "Getting There"),
        61..=80 => (Color::Yellow, "Good"),
        81..=95 => (Color::Green, "Great"),
        _ => (Color::Cyan, "Excellent"),
    };

    let bar_width = 30;
    let filled = ((score as f64 / 100.0) * bar_width as f64).round() as usize;
    let empty = bar_width - filled;
    let bar = format!(
        "{}{}",
        "█".repeat(filled).with(color),
        "░".repeat(empty).with(Color::DarkGrey)
    );

    println!("  ┌────────────────────────────────────────────────┐");
    println!(
        "  │  Harness Score   {}             │",
        format!("{score:>3} / 100").with(color).attribute(Attribute::Bold)
    );
    println!("  │  {bar}  {:<14}│", label.with(color));
    println!("  └────────────────────────────────────────────────┘");
}

fn print_section_header(title: &str) {
    println!(
        "  {} {}",
        "▸".with(Color::Cyan),
        title.with(Color::White).attribute(Attribute::Bold)
    );
    println!();
}

fn print_box(title: &str, content: &str, accent: Color) {
    let wrapped = textwrap::fill(content, 60);
    let lines: Vec<&str> = wrapped.lines().collect();

    println!(
        "  {} {}",
        "▸".with(accent),
        title.with(Color::White).attribute(Attribute::Bold)
    );
    for line in &lines {
        println!("  {}  {}", "│".with(accent), line);
    }
    println!();
}

fn severity_badge(severity: &str) -> String {
    let (color, icon) = match severity.to_uppercase().as_str() {
        "HIGH" => (Color::Red, "●"),
        "MEDIUM" => (Color::DarkYellow, "●"),
        "LOW" => (Color::Green, "●"),
        _ => (Color::DarkGrey, "●"),
    };
    format!("{} {}", icon.with(color), severity.with(color))
}

fn confidence_badge(confidence: &str) -> String {
    let color = match confidence.to_uppercase().as_str() {
        "HIGH" => Color::White,
        "MEDIUM" => Color::DarkGrey,
        _ => Color::DarkGrey,
    };
    format!("{}", confidence.to_lowercase().with(color))
}

fn print_findings(findings: &[&stage2::Finding], generated_configs: &[stage2::GeneratedConfig]) {
    for (index, finding) in findings.iter().enumerate() {
        println!(
            "  {}",
            format!("  {}. {}", index + 1, finding.title)
                .with(Color::White)
                .attribute(Attribute::Bold)
        );
        println!(
            "     {}  confidence: {}",
            severity_badge(&finding.severity),
            confidence_badge(&finding.confidence)
        );
        println!();

        let evidence_wrapped = textwrap::fill(&finding.evidence, 58);
        for line in evidence_wrapped.lines() {
            println!("     {}  {}", "│".with(Color::DarkGrey), line.with(Color::DarkGrey));
        }
        println!();

        let story_wrapped = textwrap::fill(&finding.story, 58);
        for line in story_wrapped.lines() {
            println!("     {}", line);
        }
        println!();

        let fix_wrapped = textwrap::fill(&finding.fix, 55);
        for (i, line) in fix_wrapped.lines().enumerate() {
            if i == 0 {
                println!(
                    "     {} {}",
                    "Fix:".with(Color::Green).attribute(Attribute::Bold),
                    line.with(Color::Green)
                );
            } else {
                println!("          {}", line.with(Color::Green));
            }
        }

        let impact_wrapped = textwrap::fill(&finding.impact, 52);
        for (i, line) in impact_wrapped.lines().enumerate() {
            if i == 0 {
                println!(
                    "     {} {}",
                    "Impact:".with(Color::Cyan).attribute(Attribute::Bold),
                    line.with(Color::Cyan)
                );
            } else {
                println!("             {}", line.with(Color::Cyan));
            }
        }

        if let Some(change) = generated_configs
            .iter()
            .find(|c| c.title == finding.title || c.title.contains(&finding.title))
        {
            if !change.new_text.trim().is_empty() {
                println!();
                let label = match change.scope.as_str() {
                    "user" => "Suggested workflow pattern:",
                    _ => "Proposed change:",
                };
                println!(
                    "     {} {}",
                    "▸".with(Color::DarkYellow),
                    label.with(Color::DarkYellow)
                );
                for line in change.new_text.trim().lines() {
                    println!(
                        "     {}  {}",
                        "│".with(Color::DarkYellow),
                        line.with(Color::White)
                    );
                }
            }
        }

        println!();
        if index < findings.len() - 1 {
            println!("  {}", "· · ·".with(Color::DarkGrey));
            println!();
        }
    }
}
