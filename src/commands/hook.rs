use crate::capture;
use crate::HookCommand;
use std::panic::{self, AssertUnwindSafe};

pub fn run(event: HookCommand) {
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = run_inner(event);
    }));
}

fn run_inner(event: HookCommand) -> crate::AppResult<()> {
    match event {
        HookCommand::Prompt => capture::prompt::handle_from_stdin(),
        HookCommand::Tool => capture::tool::handle_from_stdin(),
        HookCommand::Stop => capture::stop::handle_from_stdin(),
        HookCommand::SessionEnd => capture::session_end::handle_from_stdin(),
        HookCommand::SubagentStop => capture::subagent::handle_from_stdin(),
        HookCommand::PostCompact => capture::compact::handle_from_stdin(),
        HookCommand::TaskCompleted => capture::task::handle_from_stdin(),
        HookCommand::Commit {
            commit_hash,
            branch,
            project_path,
        } => capture::commit::handle(&commit_hash, branch.as_deref(), project_path.as_deref()),
    }
}
