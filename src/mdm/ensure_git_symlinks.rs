use crate::error::GitAiError;
use crate::git::repository::exec_git;
use std::path::PathBuf;

/// Ensures the libexec symlink and bash.exe shim exist for Fork compatibility.
/// Creates a symlink from <binary_parent>/../libexec to the real git's libexec,
/// and on Windows, copies bash.exe next to the git shim so Fork can validate
/// the custom git instance (Fork requires bash.exe alongside git.exe).
pub fn ensure_git_symlinks() -> Result<(), GitAiError> {
    // Get current executable path
    let exe_path = std::env::current_exe()?;

    // Skip symlink creation if running from Nix store (read-only filesystem)
    // or other read-only install locations. In these cases, the packaging system
    // (e.g., Nix flake) should handle creating the libexec symlink at build time.
    if exe_path.to_string_lossy().contains("/nix/store") {
        return Ok(());
    }

    // Get parent directories: binary_dir is e.g. ~/.git-ai/bin, base_dir is ~/.git-ai
    let binary_dir = exe_path
        .parent()
        .ok_or_else(|| GitAiError::Generic("Cannot get binary directory".to_string()))?;
    let base_dir = binary_dir
        .parent()
        .ok_or_else(|| GitAiError::Generic("Cannot get base directory".to_string()))?;

    // Get real git's exec-path (e.g. /usr/libexec/git-core)
    let output = exec_git(&["--exec-path".to_string()])?;
    let exec_path = String::from_utf8(output.stdout)?.trim().to_string();
    let exec_path = PathBuf::from(exec_path);

    // Get the libexec directory (parent of git-core)
    let libexec_target = exec_path.parent().ok_or_else(|| {
        GitAiError::Generic("Cannot get libexec directory from exec-path".to_string())
    })?;

    // Create symlink: base_dir/libexec -> /usr/libexec
    let symlink_path = base_dir.join("libexec");

    // Remove existing symlink/junction if present
    if symlink_path.exists() || symlink_path.symlink_metadata().is_ok() {
        // On Windows, junctions are directories, so use remove_dir
        #[cfg(windows)]
        {
            // Try remove_dir first (for junctions), then remove_file (for symlinks)
            if std::fs::remove_dir(&symlink_path).is_err() {
                let _ = std::fs::remove_file(&symlink_path);
            }
        }
        #[cfg(unix)]
        std::fs::remove_file(&symlink_path)?;
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(libexec_target, &symlink_path)?;

    #[cfg(windows)]
    create_junction(&symlink_path, libexec_target)?;

    // On Windows, ensure bash.exe exists next to the git shim for Fork compatibility.
    // Fork validates that bash.exe is present alongside the custom git instance and
    // rejects the configuration without it (see issue #606).
    #[cfg(windows)]
    ensure_bash_shim(binary_dir)?;

    Ok(())
}

/// Create a directory junction on Windows (doesn't require admin privileges)
#[cfg(windows)]
fn create_junction(
    junction_path: &std::path::Path,
    target: &std::path::Path,
) -> Result<(), GitAiError> {
    use std::process::Command;

    // Use mklink /J to create a junction - this doesn't require admin privileges
    let status = Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            &junction_path.to_string_lossy(),
            &target.to_string_lossy(),
        ])
        .output()
        .map_err(|e| GitAiError::Generic(format!("Failed to run mklink: {}", e)))?;

    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        return Err(GitAiError::Generic(format!(
            "Failed to create junction: {}",
            stderr
        )));
    }

    Ok(())
}

/// Ensure bash.exe exists next to the git shim on Windows.
/// Fork requires bash.exe alongside the custom git instance; without it Fork
/// rejects the configuration with "Missing bash.exe" (see issue #606).
/// We locate the real bash.exe from Git for Windows and copy it into binary_dir.
#[cfg(windows)]
fn ensure_bash_shim(binary_dir: &std::path::Path) -> Result<(), GitAiError> {
    let bash_shim = binary_dir.join("bash.exe");

    // If bash.exe already exists, nothing to do
    if bash_shim.exists() {
        return Ok(());
    }

    // Try to find the real bash.exe from Git for Windows
    if let Some(real_bash) = find_git_bash() {
        std::fs::copy(&real_bash, &bash_shim).map_err(|e| {
            GitAiError::Generic(format!(
                "Failed to copy bash.exe from {} to {}: {}",
                real_bash.display(),
                bash_shim.display(),
                e
            ))
        })?;
    }

    Ok(())
}

/// Locate bash.exe from a Git for Windows installation.
/// Checks common install locations and the system PATH.
#[cfg(windows)]
fn find_git_bash() -> Option<PathBuf> {
    use crate::git::repository::exec_git;

    // Strategy 1: Ask git for its exec-path and navigate to the bin directory
    // Git for Windows layout: <git_root>/libexec/git-core is exec-path,
    // so <git_root>/bin/bash.exe or <git_root>/usr/bin/bash.exe
    if let Ok(output) = exec_git(&["--exec-path".to_string()]) {
        if let Ok(exec_path_str) = String::from_utf8(output.stdout) {
            let exec_path = PathBuf::from(exec_path_str.trim());
            // exec-path is typically <git_root>/mingw64/libexec/git-core
            // Navigate up to find the git root
            if let Some(git_root) = exec_path.ancestors().nth(3) {
                let candidate = git_root.join("bin").join("bash.exe");
                if candidate.is_file() {
                    return Some(candidate);
                }
                let candidate = git_root.join("usr").join("bin").join("bash.exe");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    // Strategy 2: Check common Git for Windows install locations
    let program_files = std::env::var("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\Program Files"));

    for git_dir in [
        program_files.join("Git"),
        PathBuf::from(r"C:\Program Files\Git"),
        PathBuf::from(r"C:\Program Files (x86)\Git"),
    ] {
        let candidate = git_dir.join("bin").join("bash.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
        let candidate = git_dir.join("usr").join("bin").join("bash.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}
