# harn

A harness quality analyzer for Claude Code. Captures session data, measures how well your AI harness works, and generates data-driven improvements to AGENTS.md and your workflow.

## Quick Start

```bash
# Install
cargo install --path .

# Initialize in your project
cd your-project
harn init

# Set an API key for analysis (Anthropic or OpenAI)
harn config set api_key sk-ant-...
# or
harn config set openai_api_key sk-proj-...

# Use Claude Code as normal — harn captures sessions automatically

# View captured data
harn status

# Run AI-powered analysis
harn analyze

# Apply suggested AGENTS.md improvements
harn generate
```

## How It Works

```
You use Claude Code
        │
        ▼
  harn captures every session automatically
  (prompts, tool calls, outcomes, commits)
        │
        ▼
  harn analyze sends session data to an LLM
        │
        ▼
  You get a harness score + actionable fixes
        │
        ▼
  harn generate applies fixes to AGENTS.md
```

`harn init` installs lightweight hooks into `.claude/settings.json` and `.git/hooks/post-commit`. These run in the background and record session data to a local SQLite database at `~/.harn/harn.db`.

## Commands

### `harn init`

Sets up harn in the current project:

- Detects your tech stack
- Installs Claude Code hooks and git post-commit hook
- Backfills the last 30 days of session history
- Shows a quick summary of captured data

### `harn status`

Displays a dashboard of your session data:

```
harn status [--scope project|user|both]
```

Shows session counts, commit/abandon rates, trouble spots, error patterns, cost estimates, and prompt quality metrics.

### `harn analyze`

Runs AI-powered analysis on your session history:

```
harn analyze [--scope project|user|both]
```

Returns a harness score (0–100), findings with severity/confidence ratings, and ready-to-apply AGENTS.md changes. Requires an API key.

### `harn generate`

Applies the latest analysis recommendations to your AGENTS.md:

- Shows a diff preview for each change
- Backs up your current AGENTS.md before applying
- Options: **[a]** Apply, **[s]** Skip, **[e]** Edit, **[d]** Show diff

### `harn backfill`

Imports past sessions from Claude Code transcript history:

```
harn backfill [--days 30]
```

### `harn config`

Manage configuration:

```bash
harn config set <key> <value>
harn config get <key>
harn config list
harn config path
```

## Configuration

Config file: `~/.harn/config.toml`

```toml
api_key = ""
openai_api_key = ""
model = "claude-sonnet-4-20250514"
openai_model = "gpt-4.1"
idle_timeout = 300
exclude_projects = []
```

| Key | Default | Description |
|-----|---------|-------------|
| `api_key` | `""` | Anthropic API key |
| `openai_api_key` | `""` | OpenAI API key (used if Anthropic key is not set) |
| `model` | `claude-sonnet-4-20250514` | Anthropic model for analysis |
| `openai_model` | `gpt-4.1` | OpenAI model for analysis |
| `idle_timeout` | `300` | Seconds before a session is considered ended |
| `exclude_projects` | `[]` | Project paths to ignore (supports `*` wildcards) |

Environment variables `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `HARN_MODEL`, and `HARN_OPENAI_MODEL` override config file values.

## What Gets Captured

| Data | Source |
|------|--------|
| Prompts | `UserPromptSubmit` hook |
| Tool calls | `PostToolUse` hook (Read, Edit, Write, Bash, etc.) |
| Session outcomes | `Stop` and `SessionEnd` hooks |
| Commits | Git post-commit hook |
| Code acceptance rate | Diff analysis between AI output and final commit |
| Harness snapshots | AGENTS.md, CLAUDE.md, custom commands |

All data stays local in `~/.harn/harn.db`. Session data is sent to the configured LLM only when you run `harn analyze`.

## What Gets Analyzed

The analysis evaluates:

- **Prompt quality** — Are prompts specific enough? Do they reference files, provide examples, set constraints?
- **AGENTS.md effectiveness** — Are rules being followed? Are there gaps or dead rules?
- **Execution patterns** — Recurring tool failures, edits without reads, missing test runs
- **Cost efficiency** — Token usage, unnecessary iterations, context waste
- **Commit outcomes** — What percentage of sessions result in committed code vs abandoned work

## Scopes

The `--scope` flag controls what data is analyzed:

| Scope | What it includes |
|-------|-----------------|
| `project` | Sessions from the current project only |
| `user` | All sessions across all projects |
| `both` | Both views combined (default) |

## File Locations

| Path | Purpose |
|------|---------|
| `~/.harn/config.toml` | Configuration |
| `~/.harn/harn.db` | Session database |
| `.claude/settings.json` | Claude Code hooks (per-project) |
| `.git/hooks/post-commit` | Commit tracking hook |
