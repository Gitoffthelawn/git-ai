using System;
using System.Diagnostics;
using System.IO;

namespace GitAiVS.Services
{
    /// <summary>
    /// Locates the git-ai binary on the system.
    /// Search order:
    ///   1. %USERPROFILE%\.git-ai\bin\git-ai.exe  (production install)
    ///   2. %USERPROFILE%\.git-ai-local-dev\gitwrap\bin\git-ai.exe  (nix dev)
    ///   3. PATH lookup via "where git-ai"
    ///
    /// Modeled after IntelliJ's GitAiService.findGitAiBinary().
    /// </summary>
    public sealed class BinaryResolver
    {
        private static readonly Version MinVersion = new(1, 0, 23);
        private const int VersionCheckTimeoutMs = 5000;
        private const int PathLookupTimeoutMs = 5000;

        private string? _cachedPath;
        private Version? _cachedVersion;
        private string[]? _lastSearchedPaths;

        public string? ResolvedPath => _cachedPath;
        public Version? ResolvedVersion => _cachedVersion;

        public string? Resolve()
        {
            if (_cachedPath != null && File.Exists(_cachedPath))
                return _cachedPath;

            _cachedPath = null;
            _cachedVersion = null;

            var path = FindBinary();
            if (path == null)
            {
                var searched = _lastSearchedPaths != null ? string.Join(", ", _lastSearchedPaths) : "(none)";
                Trace.WriteLine("[git-ai] git-ai not found");
                Trace.WriteLine($"[git-ai]   Searched locations: {searched}");
                Trace.WriteLine("[git-ai]   To fix: Install git-ai from https://usegitai.com");
                return null;
            }

            var version = GetVersion(path);
            if (version == null)
            {
                Trace.WriteLine($"[git-ai] Could not determine git-ai version at {path}");
                return null;
            }

            if (version < MinVersion)
            {
                Trace.WriteLine($"[git-ai] git-ai version {version} is below minimum required version {MinVersion}");
                return null;
            }

            _cachedPath = path;
            _cachedVersion = version;
            Trace.WriteLine($"[git-ai] Found git-ai at {path} (version {version})");
            return path;
        }

        public void Reset()
        {
            _cachedPath = null;
            _cachedVersion = null;
        }

        private string? FindBinary()
        {
            var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
            var isWindows = Environment.OSVersion.Platform == PlatformID.Win32NT;

            _lastSearchedPaths = isWindows
                ? new[]
                {
                    Path.Combine(home, ".git-ai", "bin", "git-ai.exe"),
                    Path.Combine(home, ".git-ai-local-dev", "gitwrap", "bin", "git-ai.exe"),
                }
                : new[]
                {
                    Path.Combine(home, ".git-ai", "bin", "git-ai"),
                    Path.Combine(home, ".git-ai-local-dev", "gitwrap", "bin", "git-ai"),
                };

            foreach (var candidate in _lastSearchedPaths)
            {
                if (File.Exists(candidate))
                    return candidate;
            }

            Trace.WriteLine("[git-ai] git-ai not found in known locations, trying PATH lookup");
            return TryPathLookup(isWindows);
        }

        private static string? TryPathLookup(bool isWindows)
        {
            try
            {
                var psi = new ProcessStartInfo
                {
                    FileName = isWindows ? "cmd" : "/bin/sh",
                    Arguments = isWindows ? "/c where git-ai" : "-l -c \"which git-ai\"",
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    CreateNoWindow = true,
                };

                using var proc = Process.Start(psi);
                if (proc == null) return null;

                var output = proc.StandardOutput.ReadToEnd().Trim();
                proc.WaitForExit(PathLookupTimeoutMs);
                if (!proc.HasExited) { proc.Kill(); return null; }

                if (proc.ExitCode != 0) return null;

                var firstLine = output.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries);
                if (firstLine.Length > 0 && File.Exists(firstLine[0]))
                {
                    Trace.WriteLine($"[git-ai] Found git-ai via PATH lookup: {firstLine[0]}");
                    return firstLine[0];
                }
            }
            catch
            {
                Trace.WriteLine("[git-ai] PATH lookup for git-ai failed");
            }

            return null;
        }

        private static Version? GetVersion(string binaryPath)
        {
            try
            {
                var psi = new ProcessStartInfo
                {
                    FileName = binaryPath,
                    Arguments = "version",
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    CreateNoWindow = true,
                };

                using var proc = Process.Start(psi);
                if (proc == null) return null;

                var output = proc.StandardOutput.ReadToEnd().Trim();
                var stderr = proc.StandardError.ReadToEnd().Trim();
                proc.WaitForExit(VersionCheckTimeoutMs);

                if (!proc.HasExited)
                {
                    proc.Kill();
                    Trace.WriteLine("[git-ai] git-ai version check timed out");
                    return null;
                }

                if (proc.ExitCode != 0)
                {
                    Trace.WriteLine($"[git-ai] git-ai version check failed");
                    Trace.WriteLine($"[git-ai]   Exit code: {proc.ExitCode}");
                    Trace.WriteLine($"[git-ai]   Stdout: {output}");
                    Trace.WriteLine($"[git-ai]   Stderr: {stderr}");
                    return null;
                }

                return ParseVersion(output);
            }
            catch (Exception ex)
            {
                Trace.WriteLine($"[git-ai] git-ai version check error: {ex.Message}");
                return null;
            }
        }

        internal static Version? ParseVersion(string versionString)
        {
            var part = versionString.Trim().Split(' ')[0];
            var segments = part.Split('.');
            if (segments.Length < 3) return null;

            if (int.TryParse(segments[0], out var major)
                && int.TryParse(segments[1], out var minor)
                && int.TryParse(segments[2].Split('-', '+')[0], out var patch))
            {
                return new Version(major, minor, patch);
            }

            return null;
        }
    }
}
