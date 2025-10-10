use crate::authorship::rebase_authorship::reconstruct_working_log_after_stash_apply;
use crate::commands::hooks::commit_hooks::get_commit_default_author;
use crate::git::cli_parser::ParsedGitInvocation;
use crate::git::repository::Repository;
use crate::git::rewrite_log::{
    RewriteLogEvent, StashApplyEvent, StashCreateEvent, StashOperation, StashPopEvent,
};
use crate::git::status::EntryKind;
use crate::utils::debug_log;

pub fn pre_stash_hook(parsed_args: &ParsedGitInvocation, repository: &mut Repository) {
    debug_log("=== STASH PRE-COMMAND HOOK ===");

    let operation = match get_stash_operation(parsed_args) {
        Some(op) => op,
        None => {
            debug_log("Could not determine stash operation, skipping pre-hook");
            return;
        }
    };

    debug_log(&format!("Stash operation: {:?}", operation));

    match operation {
        StashOperation::Create => {
            // For stash push/create, capture current HEAD
            if let Ok(head) = repository.head() {
                if let Ok(target) = head.target() {
                    debug_log(&format!("Stashing from HEAD: {}", target));

                    // Log stash create event
                    let stash_event = RewriteLogEvent::stash_create(StashCreateEvent::new(
                        target.clone(),
                        None, // stash_ref not known yet
                    ));

                    match repository.storage.append_rewrite_event(stash_event) {
                        Ok(_) => debug_log("✓ Logged StashCreate event"),
                        Err(e) => debug_log(&format!("✗ Failed to log StashCreate event: {}", e)),
                    }
                }
            }
        }
        StashOperation::Apply => {
            // For stash apply, we'll log the event in post-hook when we have all info
            debug_log("Stash apply - event will be logged in post-hook");
        }
        StashOperation::Pop => {
            // For stash pop, we'll log the event in post-hook when we have all info
            debug_log("Stash pop - event will be logged in post-hook");
        }
    }
}

pub fn post_stash_hook(
    parsed_args: &ParsedGitInvocation,
    exit_status: std::process::ExitStatus,
    repository: &mut Repository,
) {
    debug_log("=== STASH POST-COMMAND HOOK ===");
    debug_log(&format!("Exit status: {}", exit_status));

    let operation = match get_stash_operation(parsed_args) {
        Some(op) => op,
        None => {
            debug_log("Could not determine stash operation, skipping post-hook");
            return;
        }
    };

    if !exit_status.success() {
        debug_log(&format!(
            "Stash {:?} failed, skipping authorship handling",
            operation
        ));
        return;
    }

    match operation {
        StashOperation::Create => {
            debug_log("✓ Stash created successfully, working log preserved");
            // Working log remains untouched at original commit
        }
        StashOperation::Apply => {
            process_stash_apply(repository, parsed_args, operation);
        }
        StashOperation::Pop => {
            process_stash_apply(repository, parsed_args, operation);
        }
    }
}

fn process_stash_apply(
    repository: &mut Repository,
    parsed_args: &ParsedGitInvocation,
    operation: StashOperation,
) {
    debug_log(&format!(
        "--- Processing stash {:?} completion ---",
        operation
    ));

    // Check for conflicts
    if let Ok(has_conflicts) = has_conflicts_in_working_dir(repository) {
        if has_conflicts {
            debug_log("⚠ Stash apply has conflicts, waiting for resolution");
            debug_log("  (Authorship will be reconstructed after conflict resolution)");
            return;
        }
    }

    // Get the stash ref that was applied
    let stash_ref = get_stash_ref_from_args(parsed_args);
    debug_log(&format!("Resolving stash ref: {}", stash_ref));

    // Resolve stash to commit SHA
    let stash_commit = match resolve_stash_to_commit(repository, &stash_ref) {
        Ok(sha) => {
            debug_log(&format!("Stash commit SHA: {}", sha));
            sha
        }
        Err(e) => {
            debug_log(&format!(
                "✗ Failed to resolve stash ref '{}': {}",
                stash_ref, e
            ));
            return;
        }
    };

    // Get original HEAD from stash commit (parent 0)
    let original_head = match get_stash_original_head(repository, &stash_commit) {
        Ok(sha) => {
            debug_log(&format!("Original HEAD from stash: {}", sha));
            sha
        }
        Err(e) => {
            debug_log(&format!("✗ Failed to get original HEAD from stash: {}", e));
            return;
        }
    };

    // Get current HEAD (target)
    let target_head = match repository.head().ok().and_then(|h| h.target().ok()) {
        Some(sha) => {
            debug_log(&format!("Current HEAD (target): {}", sha));
            sha
        }
        None => {
            debug_log("✗ Failed to get current HEAD");
            return;
        }
    };

    // Log the appropriate event now that we have all the information
    let stash_event = match operation {
        StashOperation::Apply => RewriteLogEvent::stash_apply(StashApplyEvent::new(
            stash_ref.clone(),
            original_head.clone(),
            target_head.clone(),
        )),
        StashOperation::Pop => RewriteLogEvent::stash_pop(StashPopEvent::new(
            stash_ref.clone(),
            original_head.clone(),
            target_head.clone(),
        )),
        _ => {
            debug_log("Unexpected stash operation in process_stash_apply");
            return;
        }
    };

    match repository.storage.append_rewrite_event(stash_event) {
        Ok(_) => debug_log(&format!("✓ Logged Stash{:?} event", operation)),
        Err(e) => debug_log(&format!(
            "✗ Failed to log Stash{:?} event: {}",
            operation, e
        )),
    }

    // Check if original HEAD and target HEAD are the same
    if original_head == target_head {
        debug_log("Original HEAD == Target HEAD, working log preserved automatically");
        return;
    }

    // Check if working log exists for original commit
    let original_working_log = repository
        .storage
        .working_log_for_base_commit(&original_head);
    let has_working_log = original_working_log
        .read_all_checkpoints()
        .map(|c| !c.is_empty())
        .unwrap_or(false);

    if !has_working_log {
        debug_log("No working log found for original commit, nothing to reconstruct");
        return;
    }

    debug_log(&format!(
        "Reconstructing authorship for stash apply from {} to {}",
        original_head, target_head
    ));

    // Get human author
    let human_author = get_commit_default_author(repository, &parsed_args.command_args);

    // Reconstruct working log
    match reconstruct_working_log_after_stash_apply(
        repository,
        &target_head,
        &original_head,
        &human_author,
    ) {
        Ok(_) => {
            debug_log("✓ Successfully reconstructed authorship for stash apply");
        }
        Err(e) => {
            debug_log(&format!(
                "✗ Failed to reconstruct authorship for stash apply: {}",
                e
            ));
        }
    }
}

/// Determine stash operation from command arguments
fn get_stash_operation(parsed_args: &ParsedGitInvocation) -> Option<StashOperation> {
    let args = &parsed_args.command_args;

    // git stash [push] is the default operation
    if args.is_empty() || (args.len() == 1 && args[0] == "push") {
        return Some(StashOperation::Create);
    }

    // Check first argument for operation
    match args.get(0).map(|s| s.as_str()) {
        Some("push") | Some("save") => Some(StashOperation::Create),
        Some("pop") => Some(StashOperation::Pop),
        Some("apply") => Some(StashOperation::Apply),
        // Drop and list operations have no authorship implications
        _ => None,
    }
}

/// Get stash ref from command arguments, defaults to stash@{0}
fn get_stash_ref_from_args(parsed_args: &ParsedGitInvocation) -> String {
    let args = &parsed_args.command_args;

    // For "git stash pop" or "git stash apply", check for stash ref in second position
    // git stash pop [stash@{n}]
    // git stash apply [stash@{n}]
    if args.len() >= 2 {
        let potential_ref = &args[1];
        if potential_ref.starts_with("stash@{") || potential_ref.starts_with("stash") {
            return potential_ref.clone();
        }
    }

    // Default to most recent stash
    "stash@{0}".to_string()
}

/// Resolve stash ref to commit SHA
fn resolve_stash_to_commit(
    repo: &Repository,
    stash_ref: &str,
) -> Result<String, crate::error::GitAiError> {
    debug_log(&format!("Resolving stash ref: {}", stash_ref));

    let mut args = repo.global_args_for_exec();
    args.push("rev-parse".to_string());
    args.push(stash_ref.to_string());

    let output = crate::git::repository::exec_git(&args)?;
    let sha = String::from_utf8(output.stdout)?.trim().to_string();

    if sha.is_empty() {
        return Err(crate::error::GitAiError::Generic(format!(
            "Failed to resolve stash ref: {}",
            stash_ref
        )));
    }

    Ok(sha)
}

/// Get original HEAD from stash commit (parent 0)
fn get_stash_original_head(
    repo: &Repository,
    stash_commit: &str,
) -> Result<String, crate::error::GitAiError> {
    debug_log(&format!(
        "Getting original HEAD from stash commit: {}",
        stash_commit
    ));

    let mut args = repo.global_args_for_exec();
    args.push("rev-parse".to_string());
    args.push(format!("{}^1", stash_commit)); // Parent 0 is the original HEAD

    let output = crate::git::repository::exec_git(&args)?;
    let sha = String::from_utf8(output.stdout)?.trim().to_string();

    if sha.is_empty() {
        return Err(crate::error::GitAiError::Generic(
            "Failed to get original HEAD from stash".to_string(),
        ));
    }

    Ok(sha)
}

/// Check if there are merge conflicts in the working directory
fn has_conflicts_in_working_dir(repo: &Repository) -> Result<bool, crate::error::GitAiError> {
    let statuses = repo.status()?;

    for entry in statuses {
        if entry.kind == EntryKind::Unmerged {
            return Ok(true);
        }
    }

    Ok(false)
}
