use crate::error::GitAiError;
use crate::mdm::hook_installer::{
    HookCheckResult, HookInstaller, HookInstallerParams, InstallResult, UninstallResult,
};

pub struct VisualStudioInstaller;

/// Visual Studio extension ID on the VS Marketplace.
const EXTENSION_ID: &str = "git-ai.git-ai-visualstudio";

/// Marketplace URL for manual installation fallback.
const MARKETPLACE_URL: &str =
    "https://marketplace.visualstudio.com/items?itemName=git-ai.git-ai-visualstudio";

impl HookInstaller for VisualStudioInstaller {
    fn name(&self) -> &str {
        "Visual Studio"
    }

    fn id(&self) -> &str {
        "visual-studio"
    }

    fn uses_config_hooks(&self) -> bool {
        false
    }

    fn process_names(&self) -> Vec<&str> {
        vec!["devenv"]
    }

    fn check_hooks(&self, _params: &HookInstallerParams) -> Result<HookCheckResult, GitAiError> {
        let installations = find_visual_studio_installations();

        if installations.is_empty() {
            return Ok(HookCheckResult {
                tool_installed: false,
                hooks_installed: false,
                hooks_up_to_date: false,
            });
        }

        // Check if any installation has the extension
        let any_has_extension = installations
            .iter()
            .any(|inst| is_extension_installed(inst));

        Ok(HookCheckResult {
            tool_installed: true,
            hooks_installed: any_has_extension,
            hooks_up_to_date: any_has_extension,
        })
    }

    fn install_hooks(
        &self,
        _params: &HookInstallerParams,
        _dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        // Visual Studio doesn't use config file hooks, only the VSIX extension
        Ok(None)
    }

    fn uninstall_hooks(
        &self,
        _params: &HookInstallerParams,
        _dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        Ok(None)
    }

    fn install_extras(
        &self,
        _params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Vec<InstallResult>, GitAiError> {
        let installations = find_visual_studio_installations();

        if installations.is_empty() {
            return Ok(vec![InstallResult {
                changed: false,
                diff: None,
                message: "Visual Studio: No installations detected".to_string(),
            }]);
        }

        let mut results = Vec::new();

        for inst in &installations {
            if is_extension_installed(inst) {
                results.push(InstallResult {
                    changed: false,
                    diff: None,
                    message: format!(
                        "Visual Studio {}: Extension already installed",
                        inst.display_version
                    ),
                });
                continue;
            }

            if dry_run {
                results.push(InstallResult {
                    changed: true,
                    diff: None,
                    message: format!(
                        "Visual Studio {}: Pending extension install",
                        inst.display_version
                    ),
                });
                continue;
            }

            // Attempt to install via VSIXInstaller.exe
            match install_vsix(inst) {
                Ok(true) => {
                    results.push(InstallResult {
                        changed: true,
                        diff: None,
                        message: format!(
                            "Visual Studio {}: Extension installed",
                            inst.display_version
                        ),
                    });
                }
                Ok(false) | Err(_) => {
                    results.push(InstallResult {
                        changed: false,
                        diff: None,
                        message: format!(
                            "Visual Studio {}: Unable to automatically install extension. \
                             Please install manually from: {}",
                            inst.display_version, MARKETPLACE_URL
                        ),
                    });
                }
            }
        }

        Ok(results)
    }

    fn uninstall_extras(
        &self,
        _params: &HookInstallerParams,
        _dry_run: bool,
    ) -> Result<Vec<UninstallResult>, GitAiError> {
        let installations = find_visual_studio_installations();

        if installations.is_empty() {
            return Ok(vec![]);
        }

        let mut results = Vec::new();
        for inst in &installations {
            if is_extension_installed(inst) {
                results.push(UninstallResult {
                    changed: false,
                    diff: None,
                    message: format!(
                        "Visual Studio {}: Extension must be uninstalled manually \
                         (Extensions > Manage Extensions)",
                        inst.display_version
                    ),
                });
            }
        }

        if results.is_empty() {
            results.push(UninstallResult {
                changed: false,
                diff: None,
                message: "Visual Studio: No extensions installed to uninstall".to_string(),
            });
        }

        Ok(results)
    }
}

#[derive(Debug)]
struct VsInstallation {
    install_path: String,
    display_version: String,
    instance_id: String,
}

/// Find Visual Studio installations using vswhere.exe (Windows) or return empty on other platforms.
fn find_visual_studio_installations() -> Vec<VsInstallation> {
    #[cfg(not(windows))]
    {
        Vec::new()
    }

    #[cfg(windows)]
    {
        find_visual_studio_windows()
    }
}

#[cfg(windows)]
fn find_visual_studio_windows() -> Vec<VsInstallation> {
    // vswhere.exe ships with the VS installer
    let vswhere_paths = [
        format!(
            "{}\\Microsoft Visual Studio\\Installer\\vswhere.exe",
            std::env::var("ProgramFiles(x86)").unwrap_or_default()
        ),
        format!(
            "{}\\Microsoft Visual Studio\\Installer\\vswhere.exe",
            std::env::var("ProgramFiles").unwrap_or_default()
        ),
    ];

    let vswhere = match vswhere_paths.iter().find(|p| std::path::Path::new(p).exists()) {
        Some(path) => path.clone(),
        None => {
            tracing::debug!("Visual Studio: vswhere.exe not found");
            return Vec::new();
        }
    };

    let output = match std::process::Command::new(&vswhere)
        .args([
            "-all",
            "-format",
            "json",
            "-property",
            "installationPath",
            "-property",
            "installationVersion",
            "-property",
            "instanceId",
        ])
        .output()
    {
        Ok(out) if out.status.success() => out,
        _ => {
            tracing::debug!("Visual Studio: vswhere.exe failed");
            return Vec::new();
        }
    };

    let json_str = String::from_utf8_lossy(&output.stdout);
    let entries: Vec<serde_json::Value> = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("Visual Studio: Failed to parse vswhere output: {}", e);
            return Vec::new();
        }
    };

    entries
        .iter()
        .filter_map(|entry| {
            let install_path = entry.get("installationPath")?.as_str()?.to_string();
            let version = entry.get("installationVersion")?.as_str()?.to_string();
            let instance_id = entry.get("instanceId")?.as_str()?.to_string();

            let dominated = version.starts_with("17.") || version.starts_with("18.");
            if !dominated {
                tracing::debug!(
                    "Visual Studio: Skipping version {} (only 17.x/18.x supported)",
                    version
                );
                return None;
            }

            Some(VsInstallation {
                install_path,
                display_version: version,
                instance_id,
            })
        })
        .collect()
}

/// Check if the git-ai extension is installed in a VS instance.
fn is_extension_installed(inst: &VsInstallation) -> bool {
    #[cfg(not(windows))]
    {
        let _ = inst;
        false
    }

    #[cfg(windows)]
    {
        // VS extensions install to %LOCALAPPDATA%\Microsoft\VisualStudio\<version>_<instanceId>\Extensions\
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
        let major_version = inst.display_version.split('.').next().unwrap_or("17");

        let extensions_dir = std::path::PathBuf::from(&local_app_data)
            .join("Microsoft")
            .join("VisualStudio")
            .join(format!("{}.0_{}", major_version, inst.instance_id))
            .join("Extensions");

        if !extensions_dir.exists() {
            return false;
        }

        // Walk the extensions directory looking for our extension manifest
        if let Ok(entries) = std::fs::read_dir(&extensions_dir) {
            for entry in entries.flatten() {
                let manifest = entry.path().join("extension.vsixmanifest");
                if manifest.exists() {
                    if let Ok(content) = std::fs::read_to_string(&manifest) {
                        if content.contains(EXTENSION_ID) || content.contains("GitAiVS") {
                            return true;
                        }
                    }
                }

                // Check subdirectories (extensions can be nested one level)
                if entry.path().is_dir() {
                    if let Ok(sub_entries) = std::fs::read_dir(entry.path()) {
                        for sub in sub_entries.flatten() {
                            let sub_manifest = sub.path().join("extension.vsixmanifest");
                            if sub_manifest.exists() {
                                if let Ok(content) = std::fs::read_to_string(&sub_manifest) {
                                    if content.contains(EXTENSION_ID)
                                        || content.contains("GitAiVS")
                                    {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        false
    }
}

/// Install the VSIX extension using VSIXInstaller.exe.
fn install_vsix(inst: &VsInstallation) -> Result<bool, GitAiError> {
    #[cfg(not(windows))]
    {
        let _ = inst;
        Ok(false)
    }

    #[cfg(windows)]
    {
        let vsix_installer = std::path::PathBuf::from(&inst.install_path)
            .join("Common7")
            .join("IDE")
            .join("VSIXInstaller.exe");

        if !vsix_installer.exists() {
            tracing::debug!(
                "Visual Studio: VSIXInstaller.exe not found at {}",
                vsix_installer.display()
            );
            return Ok(false);
        }

        // TODO: Download VSIX from marketplace or bundled location
        // For now, provide manual install instructions
        tracing::debug!(
            "Visual Studio: Automatic VSIX installation not yet implemented. \
             VSIXInstaller found at {}",
            vsix_installer.display()
        );
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdm::hook_installer::HookInstaller;

    #[test]
    fn test_visual_studio_installer_name() {
        let installer = VisualStudioInstaller;
        assert_eq!(installer.name(), "Visual Studio");
    }

    #[test]
    fn test_visual_studio_installer_id() {
        let installer = VisualStudioInstaller;
        assert_eq!(installer.id(), "visual-studio");
    }

    #[test]
    fn test_visual_studio_uses_no_config_hooks() {
        let installer = VisualStudioInstaller;
        assert!(!installer.uses_config_hooks());
    }

    #[test]
    fn test_visual_studio_process_names() {
        let installer = VisualStudioInstaller;
        assert_eq!(installer.process_names(), vec!["devenv"]);
    }

    #[test]
    fn test_install_hooks_returns_none() {
        let installer = VisualStudioInstaller;
        let params = HookInstallerParams {
            binary_path: std::path::PathBuf::from("/usr/local/bin/git-ai"),
        };
        let result = installer.install_hooks(&params, false).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_uninstall_hooks_returns_none() {
        let installer = VisualStudioInstaller;
        let params = HookInstallerParams {
            binary_path: std::path::PathBuf::from("/usr/local/bin/git-ai"),
        };
        let result = installer.uninstall_hooks(&params, false).unwrap();
        assert_eq!(result, None);
    }
}
