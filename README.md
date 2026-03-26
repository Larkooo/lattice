# lattice

`lattice` is a tmux-backed, tabbed interface for running and managing coding agents (`codex`, `claude`, etc.) on your own machine or VPS.

Design goals:
- top tab bar with per-instance tabs
- keyboard-first controls
- clean boxed panels with modern color theme

## Architecture

`lattice` is agent-first, but keeps runtime complexity low by using `tmux` for PTY/session durability.

- `sshd` handles remote access
- `lattice` handles agent discovery, tabs, summaries, and controls
- `tmux` handles durable sessions and attach/detach behavior

## What it does

- Auto-detects installed agent CLIs in `PATH` (currently: `codex`, `claude`, `aider`, `gemini`, `opencode`) plus custom agents via config
- Detects running agent sessions from tmux
- Spawn wizard: choose agent, navigate filesystem, create directories, clone from URL, or type a path directly
- Dashboard with instance list + summary panel showing state, PR number, CI status, branch, uptime, path, and dev server URL
- Tabbed interface with per-instance tabs and split view for side-by-side monitoring
- Tracks GitHub PR state and CI checks for agent branches (background polling)
- PR workflow: create PRs, merge PRs, fix failing CI — all from the TUI
- Git worktree support: isolate each agent in its own worktree branch
- Dev server management: auto-start companion dev servers, parse and display localhost URLs
- Startup commands: run setup commands (e.g. `pnpm install`) before agent launch
- Agent permissions: configure per-agent bypass flags
- Configurable theme, notifications (sound on completion), and refresh interval
- Title injection: agents write task titles to temp files for display in the TUI

## Quick start

1. Build:

```bash
cargo build --release
```

2. Run:

```bash
./target/release/lattice
```

3. Inside the app:

- go to the dashboard tab
- select `New Instance` in the list and press `enter`
- choose agent, then navigate folders (`..`, directories, `Create directory here...`, and `Use <path>`) and press `enter` to create
- select an instance and press `enter` to jump in
- detach from tmux normally (`Ctrl-b d`) and return to the manager

## SSH ForceCommand setup

Use a dedicated user so SSH lands directly in the manager UI.

Example (`/etc/ssh/sshd_config.d/lattice.conf`):

```text
Match User agentops
    ForceCommand /usr/local/bin/lattice
    PermitTTY yes
    X11Forwarding no
    AllowTcpForwarding no
```

Reload SSH:

```bash
sudo systemctl reload sshd
```

Then connect:

```bash
ssh agentops@your-vps
```

## Controls

### Navigation

- `up/down` (or `j/k`): move selection in lists
- `left/right` (or `h/l`, `tab`): switch tabs
- `1-9`: jump directly to instance tab by number
- `s` / `d`: return to dashboard tab
- `r`: refresh
- `q`: quit

### Instance management

- `n`: open spawn wizard (new instance)
- `enter` on an instance: attach to selected/current instance
- `x`: stop selected/current instance
- `t`: open a terminal split inside the instance's tmux session

### Spawn wizard

- `enter` in agent step: confirm agent selection
- `1-9`: select agent by number and advance
- `enter` in path step:
  - on `Use <path>`: create instance in that directory
  - on `..` or a directory: navigate
  - on `Create directory here...`: switch to directory-name input
- `.`: use current directory
- `/`: type a path directly
- `+`: create new directory
- `g`: clone from git URL
- `-` / `backspace`: go to parent directory
- `~`: jump to home directory
- `pgup/pgdn`: faster scrolling in long directory lists

### PR workflow

- `p`: create PR (no PR) / merge PR (PR open)
- `o`: open PR in browser
- `f`: ask instance to fix failing CI checks

### Split view

- `v`: enter split selection mode
- `v` (in split mode): add another pane
- `c`: remove last pane
- `enter`: launch split view (requires 2+ panes)
- `esc`: cancel split selection

### Dev servers

- `R`: start or restart the dev server for the selected instance
- `D`: stop the dev server for the selected instance

### Settings

- `enter` on settings row: open settings editor
- Settings sub-views: startup commands, dev servers, agent permissions
- Boolean settings toggle on `enter`, text settings open an inline editor
