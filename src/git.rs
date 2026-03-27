use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrChecksSummary {
    pub failed: Vec<String>,
    pub pending: usize,
    pub passed: usize,
    pub skipped: usize,
    pub cancelled: usize,
}

impl PrChecksSummary {
    pub fn has_failures(&self) -> bool {
        !self.failed.is_empty()
    }

    pub fn has_pending(&self) -> bool {
        self.pending > 0
    }

    pub fn is_empty(&self) -> bool {
        self.failed.is_empty()
            && self.pending == 0
            && self.passed == 0
            && self.skipped == 0
            && self.cancelled == 0
    }

    pub fn short_label(&self) -> Option<String> {
        if self.has_failures() {
            Some(format!(
                "{} failing{}",
                self.failed.len(),
                if self.has_pending() {
                    format!(" • {} pending", self.pending)
                } else {
                    String::new()
                }
            ))
        } else if self.has_pending() {
            Some(format!("{} pending", self.pending))
        } else if self.passed > 0 {
            Some(format!("{} passing", self.passed))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrStatus {
    pub state: Option<PrState>,
    pub number: Option<u32>,
    pub checks: Option<PrChecksSummary>,
}

#[derive(Debug, Deserialize)]
struct GhPrInfo {
    state: String,
    number: u32,
}

#[derive(Debug, Deserialize)]
struct GhPrCheck {
    bucket: String,
    name: String,
    workflow: Option<String>,
}

/// Query the GitHub CLI for PR info associated with the current branch in
/// `working_dir`. Returns empty fields if `gh` is unavailable, there is no PR
/// for this branch, or the call fails.
pub fn gh_pr_status(working_dir: &Path) -> PrStatus {
    if working_dir.as_os_str().is_empty() {
        return PrStatus { state: None, number: None, checks: None };
    }
    let output = Command::new("gh")
        .args(["pr", "view", "--json", "state,number"])
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok();

    let Some(output) = output else {
        return PrStatus { state: None, number: None, checks: None };
    };
    if !output.status.success() {
        return PrStatus { state: None, number: None, checks: None };
    }

    let info: GhPrInfo = match serde_json::from_slice(&output.stdout) {
        Ok(info) => info,
        Err(_) => {
            return PrStatus { state: None, number: None, checks: None };
        }
    };
    let state = match info.state.trim() {
        "OPEN" => Some(PrState::Open),
        "MERGED" => Some(PrState::Merged),
        "CLOSED" => Some(PrState::Closed),
        _ => None,
    };
    let checks = if state == Some(PrState::Open) { gh_pr_checks(working_dir) } else { None };

    PrStatus { state, number: Some(info.number), checks }
}

pub fn gh_pr_checks(working_dir: &Path) -> Option<PrChecksSummary> {
    let output = Command::new("gh")
        .args(["pr", "checks", "--json", "bucket,name,workflow"])
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    let code = output.status.code();
    if !output.status.success() && code != Some(8) {
        return None;
    }

    let checks: Vec<GhPrCheck> = serde_json::from_slice(&output.stdout).ok()?;
    let mut summary = PrChecksSummary::default();
    for check in checks {
        let label = match check.workflow.as_deref() {
            Some(workflow) if !workflow.is_empty() && workflow != check.name => {
                format!("{workflow}: {}", check.name)
            }
            _ => check.name,
        };

        match check.bucket.as_str() {
            "pass" => summary.passed += 1,
            "fail" => summary.failed.push(label),
            "pending" => summary.pending += 1,
            "skipping" => summary.skipped += 1,
            "cancel" => summary.cancelled += 1,
            _ => {}
        }
    }

    Some(summary)
}

/// Get the current git branch name in `working_dir`.
/// Returns an empty string if not in a git repo or on a detached HEAD.
pub fn current_branch(working_dir: &Path) -> String {
    if working_dir.as_os_str().is_empty() {
        return String::new();
    }
    let output = Command::new("git")
        .args(["-C", &working_dir.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok();
    match output {
        Some(o) if o.status.success() => {
            let branch = String::from_utf8_lossy(&o.stdout).trim().to_owned();
            if branch == "HEAD" {
                String::new()
            } else {
                branch
            }
        }
        _ => String::new(),
    }
}

/// Open the PR associated with the current branch in the default browser.
/// Runs `gh pr view --web` in the given working directory.
pub fn gh_pr_open_in_browser(working_dir: &Path) {
    let _ = Command::new("gh")
        .args(["pr", "view", "--web"])
        .current_dir(working_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Open a URL in the default browser.
pub fn open_url_in_browser(url: &str) {
    let url = normalize_browser_url(url);

    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut cmd = Command::new("open");
        cmd.arg(&url);
        cmd
    };

    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", &url]);
        cmd
    };

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut cmd = {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(&url);
        cmd
    };

    let _ = cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

fn normalize_browser_url(url: &str) -> String {
    url.replace("://0.0.0.0", "://localhost")
}

/// Check if `path` is inside a git repository.
pub fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--git-dir"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Create a worktree inside `<repo-root>/.lattice/worktrees/<short-id>/`
/// on a new branch `lattice/<short-id>` from HEAD.
/// Returns `(worktree_path, repo_root)`.
pub fn create_worktree(repo_path: &Path) -> Result<(PathBuf, PathBuf)> {
    // Find the repo root
    let output = Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git rev-parse")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("not a git repository: {}", stderr.trim());
    }

    let root = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());

    // Generate a short timestamp-based ID
    let id = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs().to_string();

    let worktree_dir = root.join(".lattice").join("worktrees");
    std::fs::create_dir_all(&worktree_dir)
        .with_context(|| format!("failed to create {}", worktree_dir.display()))?;

    let worktree_path = worktree_dir.join(&id);
    let branch_name = format!("lattice/{id}");

    let output = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            "-b",
            &branch_name,
        ])
        .output()
        .context("failed to run git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add failed: {}", stderr.trim());
    }

    Ok((worktree_path, root))
}

/// Copy untracked build artifacts (node_modules, .next, etc.) from the
/// source repo into the worktree so it doesn't need a full install step.
/// This is meant to run in a background thread after the tmux session is
/// already started, so the user isn't blocked waiting on large copies.
pub fn copy_build_artifacts(repo_root: &Path, worktree_path: &Path) {
    for dir_name in &["node_modules", ".next", ".nuxt", "dist", "build", "target/debug"] {
        let src = repo_root.join(dir_name);
        if src.is_dir() {
            let dest = worktree_path.join(dir_name);
            if !dest.exists() {
                clone_dir(&src, &dest);
            }
        }
    }
}

/// Copy a directory tree using the fastest platform-available method.
/// On macOS (APFS) this uses clonefile for near-instant copy-on-write.
/// On Linux it attempts reflinks, falling back to a regular copy.
/// Failures are silently ignored — this is best-effort optimisation.
fn clone_dir(src: &Path, dest: &Path) {
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    #[cfg(target_os = "macos")]
    let status = Command::new("cp")
        .args(["-Rc", &src.to_string_lossy(), &dest.to_string_lossy()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    #[cfg(not(target_os = "macos"))]
    let status = Command::new("cp")
        .args(["--reflink=auto", "-R", &src.to_string_lossy(), &dest.to_string_lossy()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let _ = status;
}

/// Check if `path` is inside a `.lattice/worktrees/` directory.
/// Returns `true` if the path (or any parent) contains that segment.
pub fn is_worktree_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("/.lattice/worktrees/") || s.contains("\\.lattice\\worktrees\\")
}

/// Remove a worktree and its associated branch.
/// `worktree_path` should be the path inside `.lattice/worktrees/<id>/`.
/// The branch name is derived as `lattice/<id>`.
pub fn remove_worktree(worktree_path: &Path) -> Result<()> {
    // Derive the repo root: go up from .lattice/worktrees/<id>
    // worktree_path = <root>/.lattice/worktrees/<id>
    let id = worktree_path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();

    // Find the main repo root by asking the worktree's git
    let output = Command::new("git")
        .args(["-C", &worktree_path.to_string_lossy(), "worktree", "list", "--porcelain"])
        .output()
        .context("failed to run git worktree list")?;

    let root = if output.status.success() {
        // First "worktree <path>" line is the main worktree
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find(|l| l.starts_with("worktree "))
            .map(|l| l.strip_prefix("worktree ").unwrap_or(l).to_owned())
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Remove the worktree (--force in case of uncommitted changes)
    if !root.is_empty() {
        let _ = Command::new("git")
            .args(["-C", &root, "worktree", "remove", "--force", &worktree_path.to_string_lossy()])
            .output();
    }

    // If the directory still exists (e.g. git worktree remove failed), clean up manually
    if worktree_path.exists() {
        let _ = std::fs::remove_dir_all(worktree_path);
    }

    // Delete the branch
    if !root.is_empty() && !id.is_empty() {
        let branch = format!("lattice/{id}");
        let _ = Command::new("git").args(["-C", &root, "branch", "-D", &branch]).output();
    }

    Ok(())
}

/// Install a `commit-msg` git hook that strips Co-Authored-By trailers.
/// If a hook already exists, it is preserved and chained via `exec`.
pub fn install_strip_coauthor_hook(working_dir: &Path) -> Result<()> {
    install_coauthor_hook(working_dir, false)
}

/// Install a `commit-msg` git hook that replaces any Co-Authored-By trailers
/// with a single Lattice co-author line.
/// If a hook already exists, it is preserved and chained via `exec`.
pub fn install_lattice_coauthor_hook(working_dir: &Path) -> Result<()> {
    install_coauthor_hook(working_dir, true)
}

fn install_coauthor_hook(working_dir: &Path, add_lattice: bool) -> Result<()> {
    // Find the git hooks directory for this working tree
    let output = Command::new("git")
        .args(["-C", &working_dir.to_string_lossy(), "rev-parse", "--git-path", "hooks"])
        .output()
        .context("failed to locate git hooks directory")?;

    if !output.status.success() {
        anyhow::bail!("not a git repository");
    }

    let hooks_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let hooks_dir = if hooks_dir.is_relative() { working_dir.join(hooks_dir) } else { hooks_dir };
    std::fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("failed to create hooks dir: {}", hooks_dir.display()))?;

    let hook_path = hooks_dir.join("commit-msg");
    let marker = "# lattice:coauthor";

    let lattice_line = if add_lattice {
        "printf '\\nCo-Authored-By: Lattice <lattice@users.noreply.github.com>\\n' >> \"$1\"\n"
    } else {
        ""
    };

    // Don't install twice
    if hook_path.exists() {
        let existing = std::fs::read_to_string(&hook_path).unwrap_or_default();
        if existing.contains(marker) {
            return Ok(());
        }
        // Chain existing hook: rename it and call from ours
        let backup = hooks_dir.join("commit-msg.lattice-backup");
        std::fs::rename(&hook_path, &backup)
            .context("failed to back up existing commit-msg hook")?;

        let script = format!(
            "#!/bin/sh\n\
             {marker}\n\
             # Strip agent Co-Authored-By trailers\n\
             sed '/^[[:space:]]*Co-[Aa]uthored-[Bb]y:/d' \"$1\" > \"$1.tmp\" && mv \"$1.tmp\" \"$1\"\n\
             {lattice_line}\
             # Chain to original hook\n\
             exec \"{}\" \"$@\"\n",
            backup.to_string_lossy()
        );
        std::fs::write(&hook_path, script).context("failed to write commit-msg hook")?;
    } else {
        let script = format!(
            "#!/bin/sh\n\
             {marker}\n\
             # Strip agent Co-Authored-By trailers\n\
             sed '/^[[:space:]]*Co-[Aa]uthored-[Bb]y:/d' \"$1\" > \"$1.tmp\" && mv \"$1.tmp\" \"$1\"\n\
             {lattice_line}"
        );
        std::fs::write(&hook_path, script).context("failed to write commit-msg hook")?;
    }

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&hook_path, perms)
            .context("failed to make commit-msg hook executable")?;
    }

    Ok(())
}

/// Clone `url` into `dest_dir/<repo-name>/`. Returns the clone path.
/// Repo name is derived from the URL (last path segment minus .git).
pub fn clone_repo(url: &str, dest_dir: &Path) -> Result<PathBuf> {
    let repo_name = parse_repo_name(url)?;
    let clone_path = dest_dir.join(&repo_name);

    let output = Command::new("git")
        .args(["clone", url, &clone_path.to_string_lossy()])
        .output()
        .context("failed to run git clone")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone failed: {}", stderr.trim());
    }

    Ok(clone_path)
}

/// Extract repo name from a git URL.
/// Strips trailing `.git` and takes the last path segment.
fn parse_repo_name(url: &str) -> Result<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let last_segment = trimmed
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("cannot parse repo name from URL: {url}"))?;

    let name = last_segment.strip_suffix(".git").unwrap_or(last_segment);
    if name.is_empty() {
        anyhow::bail!("cannot parse repo name from URL: {url}");
    }

    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_repo_name_https() {
        assert_eq!(parse_repo_name("https://github.com/user/repo.git").unwrap(), "repo");
    }

    #[test]
    fn parse_repo_name_https_no_git() {
        assert_eq!(parse_repo_name("https://github.com/user/repo").unwrap(), "repo");
    }

    #[test]
    fn parse_repo_name_ssh() {
        assert_eq!(parse_repo_name("git@github.com:user/repo.git").unwrap(), "repo");
    }

    #[test]
    fn parse_repo_name_trailing_slash() {
        assert_eq!(parse_repo_name("https://github.com/user/repo/").unwrap(), "repo");
    }

    #[test]
    fn parse_repo_name_empty_errors() {
        assert!(parse_repo_name("").is_err());
    }

    #[test]
    fn is_git_repo_false_for_tmp() {
        assert!(!is_git_repo(Path::new("/tmp")));
    }

    #[test]
    fn normalize_browser_url_rewrites_zero_addr() {
        assert_eq!(normalize_browser_url("http://0.0.0.0:3000"), "http://localhost:3000");
    }

    #[test]
    fn normalize_browser_url_leaves_localhost_unchanged() {
        assert_eq!(normalize_browser_url("http://localhost:5173/"), "http://localhost:5173/");
    }
}
