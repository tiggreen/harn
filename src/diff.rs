use crate::AppResult;
use similar::{ChangeTag, TextDiff};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct EditComputation {
    pub file_path: String,
    pub ai_lines: i64,
    pub human_kept: i64,
    pub human_modified: i64,
    pub human_deleted: i64,
    pub human_added: i64,
    pub acceptance_rate: Option<f64>,
}

pub fn compute_edit_for_commit(
    project_path: &Path,
    commit_hash: &str,
    file_path: &str,
) -> AppResult<Option<EditComputation>> {
    let previous = git_show(project_path, &format!("{commit_hash}~1:{file_path}"));
    let current = git_show(project_path, &format!("{commit_hash}:{file_path}"));

    let (previous, current) = match (previous, current) {
        (Ok(previous), Ok(current)) => (previous, current),
        _ => return Ok(None),
    };

    let diff = TextDiff::from_lines(&previous, &current);
    let mut changed = 0_i64;
    let mut added = 0_i64;
    let mut deleted = 0_i64;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {}
            ChangeTag::Delete => {
                changed += 1;
                deleted += 1;
            }
            ChangeTag::Insert => {
                changed += 1;
                added += 1;
            }
        }
    }

    if changed == 0 {
        return Ok(None);
    }

    Ok(Some(EditComputation {
        file_path: file_path.to_string(),
        ai_lines: changed,
        human_kept: 0,
        human_modified: 0,
        human_deleted: deleted,
        human_added: added,
        acceptance_rate: None,
    }))
}

fn git_show(project_path: &Path, object: &str) -> AppResult<String> {
    let output = Command::new("git")
        .current_dir(project_path)
        .args(["show", object])
        .output()?;

    if !output.status.success() {
        return Err(crate::boxed_error(String::from_utf8_lossy(&output.stderr)));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
