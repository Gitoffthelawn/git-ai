//! Benchmark for post-commit stats slow paths.
//!
//! This benchmark reproduces the commit-time slowdown caused by stats computation
//! on commits with many changed hunks.
//!
//! Run with:
//! `cargo test benchmark_stats_hunk_density_hotspot -- --ignored --nocapture`

use git_ai::authorship::diff_ai_accepted::diff_ai_accepted_stats;
use git_ai::authorship::stats::{get_git_diff_stats, stats_for_commit_stats};
use git_ai::git::find_repository_in_path;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[derive(Debug)]
struct StatsBreakdown {
    git_numstat: Duration,
    diff_ai_accepted: Duration,
    total_stats: Duration,
}

#[derive(Debug)]
struct CommitPerfBreakdown {
    pre_command_ms: u64,
    git_ms: u64,
    post_command_ms: u64,
    total_ms: u64,
}

fn run_git(repo_path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .expect("failed to execute git command");

    assert!(
        output.status.success(),
        "git {:?} failed:\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_lines(path: &Path, line_count: usize) {
    let content = (1..=line_count)
        .map(|i| format!("line {}\n", i))
        .collect::<String>();
    fs::write(path, content).expect("failed to write file");
}

fn mutate_file_with_scattered_replacements(path: &Path, up_to_line: usize, every_n: usize) {
    let content = fs::read_to_string(path).expect("failed to read file");
    let mut lines: Vec<String> = content.lines().map(ToString::to_string).collect();

    for (idx, line) in lines.iter_mut().enumerate() {
        let line_no = idx + 1;
        if line_no <= up_to_line && line_no % every_n == 0 {
            line.push_str(" changed");
        }
    }

    let new_content = lines.join("\n") + "\n";
    fs::write(path, new_content).expect("failed to write mutated file");
}

fn append_block(path: &Path, lines: usize) {
    let mut content = fs::read_to_string(path).expect("failed to read file");
    for i in 1..=lines {
        content.push_str(&format!("new {}\n", i));
    }
    fs::write(path, content).expect("failed to append block");
}

fn setup_repo_and_commit(case: &str) -> TempDir {
    let tmp = TempDir::new().expect("failed to create tempdir");
    let repo = tmp.path();

    run_git(repo, &["init", "-q"]);
    run_git(repo, &["config", "user.name", "Perf User"]);
    run_git(repo, &["config", "user.email", "perf@example.com"]);

    match case {
        // Many files, one contiguous added block per file (low hunk density)
        "many_files_contiguous" => {
            for i in 1..=80 {
                let path = repo.join(format!("f{}.txt", i));
                write_lines(&path, 200);
            }
            run_git(repo, &["add", "-A"]);
            run_git(repo, &["commit", "-q", "-m", "initial"]);

            for i in 1..=80 {
                let path = repo.join(format!("f{}.txt", i));
                append_block(&path, 20);
            }
        }
        // Many files, many one-line replacements per file (high hunk density)
        "many_files_scattered" => {
            for i in 1..=60 {
                let path = repo.join(format!("m{}.txt", i));
                write_lines(&path, 240);
            }
            run_git(repo, &["add", "-A"]);
            run_git(repo, &["commit", "-q", "-m", "initial"]);

            for i in 1..=60 {
                let path = repo.join(format!("m{}.txt", i));
                mutate_file_with_scattered_replacements(&path, 200, 4);
            }
        }
        _ => panic!("unknown benchmark case: {}", case),
    }

    run_git(repo, &["add", "-A"]);
    run_git(repo, &["commit", "-q", "-m", "workload"]);
    tmp
}

fn benchmark_stats(repo_path: &Path) -> StatsBreakdown {
    let repo = find_repository_in_path(repo_path.to_str().expect("non-utf8 path"))
        .expect("failed to open repository");

    let head_sha = repo
        .head()
        .expect("failed to get HEAD")
        .target()
        .expect("failed to resolve HEAD target");

    let parent_sha = repo
        .find_commit(head_sha.clone())
        .expect("failed to find HEAD commit")
        .parent(0)
        .expect("failed to find parent")
        .id();

    let git_numstat_start = Instant::now();
    let _git_numstat = get_git_diff_stats(&repo, &head_sha, &[]).expect("git numstat failed");
    let git_numstat = git_numstat_start.elapsed();

    let diff_ai_start = Instant::now();
    let _diff_ai = diff_ai_accepted_stats(&repo, &parent_sha, &head_sha, Some(&parent_sha), &[])
        .expect("diff_ai_accepted_stats failed");
    let diff_ai_accepted = diff_ai_start.elapsed();

    let total_stats_start = Instant::now();
    let _stats = stats_for_commit_stats(&repo, &head_sha, &[]).expect("stats_for_commit_stats");
    let total_stats = total_stats_start.elapsed();

    StatsBreakdown {
        git_numstat,
        diff_ai_accepted,
        total_stats,
    }
}

fn git_ai_bin() -> String {
    std::env::var("CARGO_BIN_EXE_git-ai")
        .unwrap_or_else(|_| format!("{}/target/debug/git-ai", env!("CARGO_MANIFEST_DIR")))
}

fn benchmark_commit_with_git_ai(repo_path: &Path, message: &str) -> CommitPerfBreakdown {
    let output = Command::new(git_ai_bin())
        .arg("-C")
        .arg(repo_path)
        .arg("commit")
        .arg("-m")
        .arg(message)
        .env("GIT_AI", "git")
        .env("GIT_AI_DEBUG_PERFORMANCE", "2")
        .output()
        .expect("failed to execute git-ai commit");

    assert!(
        output.status.success(),
        "git-ai commit failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let perf_json_line = combined
        .lines()
        .find(|line| line.contains("[git-ai (perf-json)]"))
        .expect("missing perf-json output from git-ai commit");

    let json_start = perf_json_line
        .find('{')
        .expect("perf-json line missing JSON payload");
    let perf_value: serde_json::Value =
        serde_json::from_str(&perf_json_line[json_start..]).expect("invalid perf JSON");

    CommitPerfBreakdown {
        pre_command_ms: perf_value["pre_command_duration_ms"].as_u64().unwrap_or(0),
        git_ms: perf_value["git_duration_ms"].as_u64().unwrap_or(0),
        post_command_ms: perf_value["post_command_duration_ms"].as_u64().unwrap_or(0),
        total_ms: perf_value["total_duration_ms"].as_u64().unwrap_or(0),
    }
}

#[test]
#[ignore] // Run manually; this is intentionally expensive.
fn benchmark_stats_hunk_density_hotspot() {
    let contiguous_repo = setup_repo_and_commit("many_files_contiguous");
    let scattered_repo = setup_repo_and_commit("many_files_scattered");

    let contiguous = benchmark_stats(contiguous_repo.path());
    let scattered = benchmark_stats(scattered_repo.path());

    println!("\n=== Stats Benchmark: Contiguous Changes ===");
    println!(
        "git numstat:          {:>8.2}ms",
        contiguous.git_numstat.as_secs_f64() * 1000.0
    );
    println!(
        "diff_ai_accepted:     {:>8.2}ms",
        contiguous.diff_ai_accepted.as_secs_f64() * 1000.0
    );
    println!(
        "total stats_for_commit_stats: {:>8.2}ms",
        contiguous.total_stats.as_secs_f64() * 1000.0
    );

    println!("\n=== Stats Benchmark: Scattered Changes ===");
    println!(
        "git numstat:          {:>8.2}ms",
        scattered.git_numstat.as_secs_f64() * 1000.0
    );
    println!(
        "diff_ai_accepted:     {:>8.2}ms",
        scattered.diff_ai_accepted.as_secs_f64() * 1000.0
    );
    println!(
        "total stats_for_commit_stats: {:>8.2}ms",
        scattered.total_stats.as_secs_f64() * 1000.0
    );

    // Sanity checks: the hotspot should dominate in the scattered case.
    assert!(scattered.total_stats > contiguous.total_stats);
    assert!(scattered.diff_ai_accepted > contiguous.diff_ai_accepted);
}

#[test]
#[ignore] // Run manually; this is intentionally expensive.
fn benchmark_commit_post_command_hunk_density_hotspot() {
    // Setup and stage contiguous case (without committing workload yet)
    let contiguous_repo = TempDir::new().expect("failed to create tempdir");
    let contiguous_path = contiguous_repo.path();
    run_git(contiguous_path, &["init", "-q"]);
    run_git(contiguous_path, &["config", "user.name", "Perf User"]);
    run_git(
        contiguous_path,
        &["config", "user.email", "perf@example.com"],
    );
    for i in 1..=80 {
        write_lines(&contiguous_path.join(format!("f{}.txt", i)), 200);
    }
    run_git(contiguous_path, &["add", "-A"]);
    run_git(contiguous_path, &["commit", "-q", "-m", "initial"]);
    for i in 1..=80 {
        append_block(&contiguous_path.join(format!("f{}.txt", i)), 20);
    }
    run_git(contiguous_path, &["add", "-A"]);

    // Setup and stage scattered case
    let scattered_repo = TempDir::new().expect("failed to create tempdir");
    let scattered_path = scattered_repo.path();
    run_git(scattered_path, &["init", "-q"]);
    run_git(scattered_path, &["config", "user.name", "Perf User"]);
    run_git(
        scattered_path,
        &["config", "user.email", "perf@example.com"],
    );
    for i in 1..=60 {
        write_lines(&scattered_path.join(format!("m{}.txt", i)), 240);
    }
    run_git(scattered_path, &["add", "-A"]);
    run_git(scattered_path, &["commit", "-q", "-m", "initial"]);
    for i in 1..=60 {
        mutate_file_with_scattered_replacements(
            &scattered_path.join(format!("m{}.txt", i)),
            200,
            4,
        );
    }
    run_git(scattered_path, &["add", "-A"]);

    let contiguous_perf = benchmark_commit_with_git_ai(contiguous_path, "contiguous");
    let scattered_perf = benchmark_commit_with_git_ai(scattered_path, "scattered");

    println!("\n=== Commit Benchmark: Contiguous Changes ===");
    println!("pre_command:  {}ms", contiguous_perf.pre_command_ms);
    println!("git command:  {}ms", contiguous_perf.git_ms);
    println!("post_command: {}ms", contiguous_perf.post_command_ms);
    println!("total:        {}ms", contiguous_perf.total_ms);

    println!("\n=== Commit Benchmark: Scattered Changes ===");
    println!("pre_command:  {}ms", scattered_perf.pre_command_ms);
    println!("git command:  {}ms", scattered_perf.git_ms);
    println!("post_command: {}ms", scattered_perf.post_command_ms);
    println!("total:        {}ms", scattered_perf.total_ms);

    assert!(contiguous_perf.total_ms > 0);
    assert!(scattered_perf.total_ms > 0);
}
