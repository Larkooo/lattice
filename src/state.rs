//! Persistent state for Lattice instances.
//!
//! Lattice's "source of truth" used to be tmux: every refresh shelled out to
//! `tmux list-sessions` and rebuilt the world from scratch. That's expensive
//! (one process spawn per session per refresh) and brittle — when the tmux
//! server dies, the entire UI goes blank even though the worktrees on disk
//! are perfectly recoverable.
//!
//! This module flips the model: every spawn/kill writes to a JSON state file
//! at `~/.config/lattice/state.json`, and refreshes read from it. Tmux is
//! still consulted for *liveness* (one `list-sessions` call per tick) and for
//! ephemeral pane data (current command, preview, pane title) for live
//! sessions only. Dormant sessions need zero filesystem walks because their
//! metadata is already in the state file.
//!
//! Writes are atomic: write to `state.json.tmp`, fsync, rename. Only the main
//! thread mutates state — background threads send results over channels and
//! the main thread applies them.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const CURRENT_VERSION: u32 = 1;

/// Top-level on-disk state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    #[serde(default)]
    pub instances: Vec<PersistedInstance>,
}

impl Default for State {
    fn default() -> Self {
        Self { version: CURRENT_VERSION, instances: Vec::new() }
    }
}

/// One persisted Lattice instance — the metadata that survives across
/// refreshes, restarts, and reboots.
///
/// Anything ephemeral (current pane command, capture-pane preview, attach
/// state) is intentionally NOT stored here — those come from tmux at refresh
/// time and only for sessions that are currently live.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedInstance {
    /// tmux session name — the unique identifier across the system.
    pub session_name: String,
    /// Agent id ("claude", "codex", "aider", ...).
    pub agent_id: String,
    /// Absolute path the agent was launched in (the worktree dir, if any).
    pub worktree_path: String,
    /// Repo root for `worktree_path`. None when the instance was launched
    /// outside a worktree (e.g. with worktrees disabled).
    #[serde(default)]
    pub repo_root: Option<String>,
    /// Unix epoch seconds when the instance was created.
    pub created_at: u64,
    /// Last known Claude `--resume` UUID for this worktree, refreshed when
    /// the agent starts a new conversation. Used to bring back the right
    /// transcript when resuming a dormant instance.
    #[serde(default)]
    pub claude_session_id: Option<String>,
    /// Last known git branch — refreshed in the background.
    #[serde(default)]
    pub branch: String,
    /// Last known agent-written title (mirror of `/tmp/lattice_<name>.title`).
    #[serde(default)]
    pub title: String,
    /// Companion dev server tmux session name, if one was started for this
    /// instance via the dev-servers config.
    #[serde(default)]
    pub dev_server_session: Option<String>,
    /// Cached PR state. Strings rather than enums so the schema is forward-
    /// compatible if we add states later.
    #[serde(default)]
    pub pr_state: Option<String>,
    #[serde(default)]
    pub pr_number: Option<u32>,
}

pub fn state_path() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| ".".to_owned());
    PathBuf::from(home).join(".config").join("lattice").join("state.json")
}

/// Load state from disk. Returns an empty state if the file doesn't exist
/// or is unreadable — missing state is normal on first launch.
pub fn load() -> State {
    let path = state_path();
    let Ok(contents) = fs::read_to_string(&path) else {
        return State::default();
    };
    match serde_json::from_str::<State>(&contents) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("lattice: warning: failed to parse {}: {err}", path.display());
            State::default()
        }
    }
}

/// Atomically write state to disk: serialise → write to `<path>.tmp` →
/// fsync → rename. A crash between any two steps leaves the previous
/// file intact.
pub fn save(state: &State) -> Result<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(state).context("serialising state")?;
    {
        let mut f = fs::File::create(&tmp)
            .with_context(|| format!("creating {}", tmp.display()))?;
        f.write_all(&body).context("writing state")?;
        f.sync_all().context("fsyncing state")?;
    }
    fs::rename(&tmp, &path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

impl State {
    /// Insert a new instance. If one with the same session name already
    /// exists, replace it.
    pub fn upsert(&mut self, inst: PersistedInstance) {
        if let Some(slot) = self.instances.iter_mut().find(|i| i.session_name == inst.session_name)
        {
            *slot = inst;
        } else {
            self.instances.push(inst);
        }
    }

    /// Remove the instance with the given session name. No-op if absent.
    pub fn remove(&mut self, session_name: &str) {
        self.instances.retain(|i| i.session_name != session_name);
    }

    pub fn get(&self, session_name: &str) -> Option<&PersistedInstance> {
        self.instances.iter().find(|i| i.session_name == session_name)
    }

    pub fn get_mut(&mut self, session_name: &str) -> Option<&mut PersistedInstance> {
        self.instances.iter_mut().find(|i| i.session_name == session_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PersistedInstance {
        PersistedInstance {
            session_name: "lattice_claude_123".to_owned(),
            agent_id: "claude".to_owned(),
            worktree_path: "/tmp/foo/.lattice/worktrees/123".to_owned(),
            repo_root: Some("/tmp/foo".to_owned()),
            created_at: 1_700_000_000,
            claude_session_id: Some("abc-uuid".to_owned()),
            branch: "lattice/123".to_owned(),
            title: "fix bug".to_owned(),
            dev_server_session: None,
            pr_state: None,
            pr_number: None,
        }
    }

    #[test]
    fn upsert_inserts_then_replaces() {
        let mut state = State::default();
        state.upsert(fixture());
        assert_eq!(state.instances.len(), 1);
        assert_eq!(state.instances[0].title, "fix bug");

        let mut updated = fixture();
        updated.title = "ship feature".to_owned();
        state.upsert(updated);
        assert_eq!(state.instances.len(), 1);
        assert_eq!(state.instances[0].title, "ship feature");
    }

    #[test]
    fn remove_drops_matching() {
        let mut state = State::default();
        state.upsert(fixture());
        state.remove("lattice_claude_123");
        assert!(state.instances.is_empty());
    }

    #[test]
    fn round_trip_through_json() {
        let mut state = State::default();
        state.upsert(fixture());
        let body = serde_json::to_string(&state).unwrap();
        let parsed: State = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed.instances.len(), 1);
        assert_eq!(parsed.instances[0].session_name, "lattice_claude_123");
        assert_eq!(parsed.instances[0].claude_session_id.as_deref(), Some("abc-uuid"));
    }

    #[test]
    fn missing_optional_fields_default() {
        // A v0-ish blob with only required fields should still parse.
        let body = r#"{
            "version": 1,
            "instances": [{
                "session_name": "lattice_claude_999",
                "agent_id": "claude",
                "worktree_path": "/tmp/foo",
                "created_at": 0
            }]
        }"#;
        let parsed: State = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.instances.len(), 1);
        assert!(parsed.instances[0].claude_session_id.is_none());
        assert!(parsed.instances[0].branch.is_empty());
    }
}
