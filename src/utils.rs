use crate::error::GitAiError;
use crate::git::diff_tree_to_tree::Diff;
use std::io::IsTerminal;
use std::path::PathBuf;

/// Check if debug logging is enabled via environment variable
///
/// This is checked once at module initialization to avoid repeated environment variable lookups.
static DEBUG_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static DEBUG_PERFORMANCE_LEVEL: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
static IS_TERMINAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

fn is_debug_enabled() -> bool {
    *DEBUG_ENABLED.get_or_init(|| {
        (cfg!(debug_assertions)
            || std::env::var("GIT_AI_DEBUG").unwrap_or_default() == "1"
            || std::env::var("GIT_AI_DEBUG_PERFORMANCE").unwrap_or_default() != "")
            && std::env::var("GIT_AI_DEBUG").unwrap_or_default() != "0"
    })
}

fn is_debug_performance_enabled() -> bool {
    debug_performance_level() >= 1
}

fn debug_performance_level() -> u8 {
    *DEBUG_PERFORMANCE_LEVEL.get_or_init(|| {
        std::env::var("GIT_AI_DEBUG_PERFORMANCE")
            .unwrap_or_default()
            .parse::<u8>()
            .unwrap_or(0)
    })
}

pub fn debug_performance_log(msg: &str) {
    if is_debug_performance_enabled() {
        eprintln!("\x1b[1;33m[git-ai (perf)]\x1b[0m {}", msg);
    }
}

pub fn debug_performance_log_structured(json: serde_json::Value) {
    if debug_performance_level() >= 2 {
        eprintln!("\x1b[1;33m[git-ai (perf-json)]\x1b[0m {}", json);
    }
}

/// Debug logging utility function
///
/// Prints debug messages with a colored prefix when debug assertions are enabled or when
/// the `GIT_AI_DEBUG` environment variable is set to "1".
///
/// # Arguments
///
/// * `msg` - The debug message to print
pub fn debug_log(msg: &str) {
    if is_debug_enabled() {
        eprintln!("\x1b[1;33m[git-ai]\x1b[0m {}", msg);
    }
}

/// Print a git diff in a readable format
///
/// Prints the diff between two commits/trees showing which files changed and their status.
/// This is useful for debugging and understanding what changes occurred.
///
/// # Arguments
///
/// * `diff` - The git diff object to print
/// * `old_label` - Label for the "old" side (e.g., commit SHA or description)
/// * `new_label` - Label for the "new" side (e.g., commit SHA or description)
pub fn _print_diff(diff: &Diff, old_label: &str, new_label: &str) {
    println!("Diff between {} and {}:", old_label, new_label);

    let mut file_count = 0;
    for delta in diff.deltas() {
        file_count += 1;
        let old_file = delta.old_file().path().unwrap_or(std::path::Path::new(""));
        let new_file = delta.new_file().path().unwrap_or(std::path::Path::new(""));
        let status = delta.status();

        println!(
            "  File {}: {} -> {} (status: {:?})",
            file_count,
            old_file.display(),
            new_file.display(),
            status
        );
    }

    if file_count == 0 {
        println!("  No changes between {} and {}", old_label, new_label);
    }
}

#[inline]
pub fn normalize_to_posix(path: &str) -> String {
    path.replace('\\', "/")
}

pub fn current_git_ai_exe() -> Result<PathBuf, GitAiError> {
    let path = std::env::current_exe()?;

    // Get platform-specific executable names
    let git_name = if cfg!(windows) { "git.exe" } else { "git" };
    let git_ai_name = if cfg!(windows) {
        "git-ai.exe"
    } else {
        "git-ai"
    };

    // Check if the filename matches the git executable name for this platform
    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
        if file_name == git_name {
            // Try replacing with git-ai executable name for this platform
            let git_ai_path = path.with_file_name(git_ai_name);

            // Check if the git-ai file exists
            if git_ai_path.exists() {
                return Ok(git_ai_path);
            }

            // If it doesn't exist, return the git-ai executable name as a PathBuf
            return Ok(PathBuf::from(git_ai_name));
        }
    }

    Ok(path)
}

pub fn is_interactive_terminal() -> bool {
    *IS_TERMINAL.get_or_init(|| std::io::stdin().is_terminal())
}

/// Unescape a git-quoted path that may contain octal escape sequences.
///
/// Git quotes filenames containing non-ASCII characters (and some special characters)
/// using C-style escaping with octal sequences. For example, a Chinese filename like
/// "中文.txt" would appear as `"\344\270\255\346\226\207.txt"` in git output.
///
/// This function handles:
/// - Quoted paths: removes surrounding quotes and unescapes content
/// - Octal escapes: converts `\NNN` sequences back to UTF-8 bytes
/// - Other escapes: `\\`, `\"`, `\n`, `\t`, etc.
/// - Unquoted paths: returned as-is
///
/// # Examples
///
/// ```
/// use git_ai::utils::unescape_git_path;
///
/// // Unquoted path - returned as-is
/// assert_eq!(unescape_git_path("simple.txt"), "simple.txt");
///
/// // Quoted path with spaces
/// assert_eq!(unescape_git_path("\"path with spaces.txt\""), "path with spaces.txt");
///
/// // Chinese characters encoded as octal
/// assert_eq!(unescape_git_path("\"\\344\\270\\255\\346\\226\\207.txt\""), "中文.txt");
/// ```
pub fn unescape_git_path(path: &str) -> String {
    // If not quoted, return as-is
    if !path.starts_with('"') || !path.ends_with('"') {
        return path.to_string();
    }

    // Remove surrounding quotes
    let inner = &path[1..path.len() - 1];

    // Parse escape sequences and collect bytes
    let mut bytes: Vec<u8> = Vec::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('\\') => {
                    chars.next();
                    bytes.push(b'\\');
                }
                Some('"') => {
                    chars.next();
                    bytes.push(b'"');
                }
                Some('n') => {
                    chars.next();
                    bytes.push(b'\n');
                }
                Some('t') => {
                    chars.next();
                    bytes.push(b'\t');
                }
                Some('r') => {
                    chars.next();
                    bytes.push(b'\r');
                }
                Some(d) if d.is_ascii_digit() => {
                    // Octal escape sequence: \NNN (1-3 octal digits)
                    let mut octal = String::new();
                    for _ in 0..3 {
                        if let Some(&d) = chars.peek() {
                            if d.is_ascii_digit() && d <= '7' {
                                octal.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    if let Ok(byte_val) = u8::from_str_radix(&octal, 8) {
                        bytes.push(byte_val);
                    }
                }
                _ => {
                    // Unknown escape - keep the backslash
                    bytes.push(b'\\');
                }
            }
        } else {
            // Regular character - encode as UTF-8
            let mut buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut buf);
            bytes.extend_from_slice(encoded.as_bytes());
        }
    }

    // Convert bytes to UTF-8 string
    String::from_utf8(bytes).unwrap_or_else(|e| {
        // If invalid UTF-8, try lossy conversion
        String::from_utf8_lossy(e.as_bytes()).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unescape_git_path_simple() {
        // Unquoted path - no change
        assert_eq!(unescape_git_path("simple.txt"), "simple.txt");
        assert_eq!(unescape_git_path("path/to/file.rs"), "path/to/file.rs");
    }

    #[test]
    fn test_unescape_git_path_quoted_with_spaces() {
        // Quoted path with spaces
        assert_eq!(
            unescape_git_path("\"path with spaces.txt\""),
            "path with spaces.txt"
        );
        assert_eq!(
            unescape_git_path("\"dir name/file name.txt\""),
            "dir name/file name.txt"
        );
    }

    #[test]
    fn test_unescape_git_path_chinese_characters() {
        // Chinese characters "中文" encoded as octal: \344\270\255\346\226\207
        assert_eq!(
            unescape_git_path("\"\\344\\270\\255\\346\\226\\207.txt\""),
            "中文.txt"
        );

        // More complex Chinese filename: "中文文件.txt"
        // 中 = \344\270\255, 文 = \346\226\207, 件 = \344\273\266
        assert_eq!(
            unescape_git_path(
                "\"\\344\\270\\255\\346\\226\\207\\346\\226\\207\\344\\273\\266.txt\""
            ),
            "中文文件.txt"
        );
    }

    #[test]
    fn test_unescape_git_path_emoji() {
        // Emoji "🚀" (rocket) = U+1F680 = \360\237\232\200 in octal UTF-8
        assert_eq!(
            unescape_git_path("\"\\360\\237\\232\\200.txt\""),
            "🚀.txt"
        );

        // Emoji "😀" (grinning face) = U+1F600 = \360\237\230\200 in octal UTF-8
        assert_eq!(
            unescape_git_path("\"\\360\\237\\230\\200.txt\""),
            "😀.txt"
        );

        // Mixed: "test_🎉_file.txt" where 🎉 = \360\237\216\211
        assert_eq!(
            unescape_git_path("\"test_\\360\\237\\216\\211_file.txt\""),
            "test_🎉_file.txt"
        );
    }

    #[test]
    fn test_unescape_git_path_escaped_characters() {
        // Escaped backslash
        assert_eq!(unescape_git_path("\"path\\\\with\\\\slashes\""), "path\\with\\slashes");

        // Escaped quotes
        assert_eq!(unescape_git_path("\"file\\\"name.txt\""), "file\"name.txt");

        // Escaped newline and tab
        assert_eq!(unescape_git_path("\"line1\\nline2\""), "line1\nline2");
        assert_eq!(unescape_git_path("\"col1\\tcol2\""), "col1\tcol2");
    }

    #[test]
    fn test_unescape_git_path_mixed_content() {
        // Mix of ASCII, Chinese, and escapes
        assert_eq!(
            unescape_git_path("\"src/\\344\\270\\255\\346\\226\\207/file.txt\""),
            "src/中文/file.txt"
        );
    }

    // =========================================================================
    // Phase 1: CJK Extended Coverage Tests
    // =========================================================================

    #[test]
    fn test_unescape_japanese_hiragana() {
        // Japanese Hiragana "ひらがな" = \343\201\262\343\202\211\343\201\214\343\201\252
        assert_eq!(
            unescape_git_path("\"\\343\\201\\262\\343\\202\\211\\343\\201\\214\\343\\201\\252.txt\""),
            "ひらがな.txt"
        );
    }

    #[test]
    fn test_unescape_japanese_katakana() {
        // Japanese Katakana "カタカナ" = \343\202\253\343\202\277\343\202\253\343\203\212
        assert_eq!(
            unescape_git_path("\"\\343\\202\\253\\343\\202\\277\\343\\202\\253\\343\\203\\212.txt\""),
            "カタカナ.txt"
        );
    }

    #[test]
    fn test_unescape_korean_hangul() {
        // Korean Hangul "한글" = \355\225\234\352\270\200
        assert_eq!(
            unescape_git_path("\"\\355\\225\\234\\352\\270\\200.txt\""),
            "한글.txt"
        );
    }

    #[test]
    fn test_unescape_traditional_chinese() {
        // Traditional Chinese "繁體" = \347\271\201\351\253\224
        assert_eq!(
            unescape_git_path("\"\\347\\271\\201\\351\\253\\224.txt\""),
            "繁體.txt"
        );
    }

    #[test]
    fn test_unescape_mixed_cjk() {
        // Mixed CJK: "日中韓" (Japanese, Chinese, Korean characters mixed)
        // 日 = \346\227\245, 中 = \344\270\255, 韓 = \351\237\223
        assert_eq!(
            unescape_git_path("\"\\346\\227\\245\\344\\270\\255\\351\\237\\223.txt\""),
            "日中韓.txt"
        );
    }

    // =========================================================================
    // Phase 2: RTL Scripts Tests (Arabic, Hebrew, Persian, Urdu)
    // =========================================================================

    #[test]
    fn test_unescape_arabic() {
        // Arabic "مرحبا" (marhaba = hello)
        // م = \331\205, ر = \330\261, ح = \330\255, ب = \330\250, ا = \330\247
        assert_eq!(
            unescape_git_path("\"\\331\\205\\330\\261\\330\\255\\330\\250\\330\\247.txt\""),
            "مرحبا.txt"
        );
    }

    #[test]
    fn test_unescape_hebrew() {
        // Hebrew "שלום" (shalom = hello/peace)
        // ש = \327\251, ל = \327\234, ו = \327\225, ם = \327\235
        assert_eq!(
            unescape_git_path("\"\\327\\251\\327\\234\\327\\225\\327\\235.txt\""),
            "שלום.txt"
        );
    }

    #[test]
    fn test_unescape_persian() {
        // Persian "فارسی" (farsi)
        // ف = \331\201, ا = \330\247, ر = \330\261, س = \330\263, ی = \333\214
        assert_eq!(
            unescape_git_path("\"\\331\\201\\330\\247\\330\\261\\330\\263\\333\\214.txt\""),
            "فارسی.txt"
        );
    }

    #[test]
    fn test_unescape_urdu() {
        // Urdu "اردو" (urdu)
        // ا = \330\247, ر = \330\261, د = \330\257, و = \331\210
        assert_eq!(
            unescape_git_path("\"\\330\\247\\330\\261\\330\\257\\331\\210.txt\""),
            "اردو.txt"
        );
    }

    #[test]
    fn test_unescape_mixed_rtl_ltr() {
        // Mixed RTL/LTR: "test_مرحبا_file" (ASCII + Arabic + ASCII)
        assert_eq!(
            unescape_git_path("\"test_\\331\\205\\330\\261\\330\\255\\330\\250\\330\\247_file.txt\""),
            "test_مرحبا_file.txt"
        );
    }
}
