use crate::commands::hooks::cherry_pick_hooks;
use crate::commands::hooks::commit_hooks;
use crate::commands::hooks::fetch_hooks;
use crate::commands::hooks::merge_hooks;
use crate::commands::hooks::push_hooks;
use crate::commands::hooks::rebase_hooks;
use crate::commands::hooks::reset_hooks;
use crate::commands::hooks::stash_hooks;
use crate::config;
use crate::git::cli_parser::{ParsedGitInvocation, parse_git_cli_args};
use crate::git::find_repository;
use crate::git::repository::Repository;

use crate::utils::debug_log;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering};

#[cfg(unix)]
static CHILD_PGID: AtomicI32 = AtomicI32::new(0);

#[cfg(unix)]
extern "C" fn forward_signal_handler(sig: libc::c_int) {
    let pgid = CHILD_PGID.load(Ordering::Relaxed);
    if pgid > 0 {
        unsafe {
            // Send to the whole child process group
            let _ = libc::kill(-pgid, sig);
        }
    }
}

#[cfg(unix)]
fn install_forwarding_handlers() {
    unsafe {
        let handler = forward_signal_handler as usize;
        let _ = libc::signal(libc::SIGTERM, handler);
        let _ = libc::signal(libc::SIGINT, handler);
        let _ = libc::signal(libc::SIGHUP, handler);
        let _ = libc::signal(libc::SIGQUIT, handler);
    }
}

#[cfg(unix)]
fn uninstall_forwarding_handlers() {
    unsafe {
        let _ = libc::signal(libc::SIGTERM, libc::SIG_DFL);
        let _ = libc::signal(libc::SIGINT, libc::SIG_DFL);
        let _ = libc::signal(libc::SIGHUP, libc::SIG_DFL);
        let _ = libc::signal(libc::SIGQUIT, libc::SIG_DFL);
    }
}

pub struct CommandHooksContext {
    pub pre_commit_hook_result: Option<bool>,
    pub rebase_original_head: Option<String>,
    pub _rebase_onto: Option<String>,
}

pub fn handle_git(args: &[String]) {
    // If we're being invoked from a shell completion context, bypass git-ai logic
    // and delegate directly to the real git so existing completion scripts work.
    if in_shell_completion_context() {
        let orig_args: Vec<String> = std::env::args().skip(1).collect();
        proxy_to_git(&orig_args, true);
        return;
    }

    let mut command_hooks_context = CommandHooksContext {
        pre_commit_hook_result: None,
        rebase_original_head: None,
        _rebase_onto: None,
    };

    let parsed_args = parse_git_cli_args(args);

    let mut repository_option = find_repository(&parsed_args.global_args).ok();

    let has_repo = repository_option.is_some();

    // println!("command: {:?}", parsed_args.command);
    // println!("global_args: {:?}", parsed_args.global_args);
    // println!("command_args: {:?}", parsed_args.command_args);
    // println!("to_invocation_vec: {:?}", parsed_args.to_invocation_vec());

    let config = config::Config::get();

    let skip_hooks = !config.is_allowed_repository(&repository_option);
    if skip_hooks {
        debug_log(
            "Skipping git-ai hooks because repository does not have at least one remote in allow_repositories list",
        );
    }

    // run with hooks
    let exit_status = if !parsed_args.is_help && has_repo && !skip_hooks {
        let repository = repository_option.as_mut().unwrap();
        run_pre_command_hooks(&mut command_hooks_context, &parsed_args, repository);
        let exit_status = proxy_to_git(&parsed_args.to_invocation_vec(), false);

        run_post_command_hooks(
            &mut command_hooks_context,
            &parsed_args,
            exit_status,
            repository,
        );
        exit_status
    } else {
        // run without hooks
        proxy_to_git(&parsed_args.to_invocation_vec(), false)
    };
    exit_with_status(exit_status);
}

fn run_pre_command_hooks(
    command_hooks_context: &mut CommandHooksContext,
    parsed_args: &ParsedGitInvocation,
    repository: &mut Repository,
) {
    // Pre-command hooks
    match parsed_args.command.as_deref() {
        Some("commit") => {
            command_hooks_context.pre_commit_hook_result = Some(
                commit_hooks::commit_pre_command_hook(parsed_args, repository),
            );
        }
        Some("rebase") => {
            rebase_hooks::pre_rebase_hook(parsed_args, repository, command_hooks_context);
        }
        Some("reset") => {
            reset_hooks::pre_reset_hook(parsed_args, repository);
        }
        Some("cherry-pick") => {
            cherry_pick_hooks::pre_cherry_pick_hook(parsed_args, repository, command_hooks_context);
        }
        Some("stash") => {
            stash_hooks::pre_stash_hook(parsed_args, repository);
        }
        _ => {}
    }
}

fn run_post_command_hooks(
    command_hooks_context: &mut CommandHooksContext,
    parsed_args: &ParsedGitInvocation,
    exit_status: std::process::ExitStatus,
    repository: &mut Repository,
) {
    // Post-command hooks
    match parsed_args.command.as_deref() {
        Some("commit") => commit_hooks::commit_post_command_hook(
            parsed_args,
            exit_status,
            repository,
            command_hooks_context,
        ),
        Some("fetch") => fetch_hooks::fetch_post_command_hook(parsed_args, exit_status),
        Some("push") => push_hooks::push_post_command_hook(parsed_args, exit_status),
        Some("reset") => reset_hooks::post_reset_hook(parsed_args, repository, exit_status),
        Some("merge") => merge_hooks::post_merge_hook(parsed_args, exit_status, repository),
        Some("rebase") => rebase_hooks::handle_rebase_post_command(
            command_hooks_context,
            parsed_args,
            exit_status,
            repository,
        ),
        Some("cherry-pick") => cherry_pick_hooks::post_cherry_pick_hook(
            command_hooks_context,
            parsed_args,
            exit_status,
            repository,
        ),
        Some("stash") => stash_hooks::post_stash_hook(parsed_args, exit_status, repository),
        _ => {}
    }
}

fn proxy_to_git(args: &[String], exit_on_completion: bool) -> std::process::ExitStatus {
    // debug_log(&format!("proxying to git with args: {:?}", args));
    // debug_log(&format!("prepended global args: {:?}", prepend_global(args)));
    // Use spawn for interactive commands
    let child = {
        #[cfg(unix)]
        {
            // Only create a new process group for non-interactive runs.
            // If stdin is a TTY, the child must remain in the foreground
            // terminal process group to avoid SIGTTIN/SIGTTOU hangs.
            let is_interactive = unsafe { libc::isatty(libc::STDIN_FILENO) == 1 };
            let should_setpgid = !is_interactive;

            let mut cmd = Command::new(config::Config::get().git_cmd());
            cmd.args(args);
            unsafe {
                let setpgid_flag = should_setpgid;
                cmd.pre_exec(move || {
                    if setpgid_flag {
                        // Make the child its own process group leader so we can signal the group
                        let _ = libc::setpgid(0, 0);
                    }
                    Ok(())
                });
            }
            // We return both the spawned child and whether we changed PGID
            match cmd.spawn() {
                Ok(child) => Ok((child, should_setpgid)),
                Err(e) => Err(e),
            }
        }
        #[cfg(not(unix))]
        {
            Command::new(config::Config::get().git_cmd())
                .args(args)
                .spawn()
        }
    };

    #[cfg(unix)]
    match child {
        Ok((mut child, setpgid)) => {
            #[cfg(unix)]
            {
                if setpgid {
                    // Record the child's process group id (same as its pid after setpgid)
                    let pgid: i32 = child.id() as i32;
                    CHILD_PGID.store(pgid, Ordering::Relaxed);
                    install_forwarding_handlers();
                }
            }
            let status = child.wait();
            match status {
                Ok(status) => {
                    #[cfg(unix)]
                    {
                        if setpgid {
                            CHILD_PGID.store(0, Ordering::Relaxed);
                            uninstall_forwarding_handlers();
                        }
                    }
                    if exit_on_completion {
                        exit_with_status(status);
                    }
                    return status;
                }
                Err(e) => {
                    #[cfg(unix)]
                    {
                        if setpgid {
                            CHILD_PGID.store(0, Ordering::Relaxed);
                            uninstall_forwarding_handlers();
                        }
                    }
                    eprintln!("Failed to wait for git process: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to execute git command: {}", e);
            std::process::exit(1);
        }
    }

    #[cfg(not(unix))]
    match child {
        Ok(mut child) => {
            let status = child.wait();
            match status {
                Ok(status) => {
                    if exit_on_completion {
                        exit_with_status(status);
                    }
                    return status;
                }
                Err(e) => {
                    eprintln!("Failed to wait for git process: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to execute git command: {}", e);
            std::process::exit(1);
        }
    }
}

// Exit mirroring the child's termination: same signal if signaled, else exit code
fn exit_with_status(status: std::process::ExitStatus) -> ! {
    #[cfg(unix)]
    {
        if let Some(sig) = status.signal() {
            unsafe {
                libc::signal(sig, libc::SIG_DFL);
                libc::raise(sig);
            }
            // Should not return
            unreachable!();
        }
    }
    std::process::exit(status.code().unwrap_or(1));
}

// Detect if current process invocation is coming from shell completion machinery
// (bash, zsh via bashcompinit). If so, we should proxy directly to the real git
// without any extra behavior that could interfere with completion scripts.
fn in_shell_completion_context() -> bool {
    std::env::var("COMP_LINE").is_ok()
        || std::env::var("COMP_POINT").is_ok()
        || std::env::var("COMP_TYPE").is_ok()
}
