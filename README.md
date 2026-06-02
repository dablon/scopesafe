# scopesafe — AI Agent Scope Guardrail

**Your agent has a scope. It should respect it.**

scopesafe is a CLI tool and MCP server that defines boundaries for AI coding agents (Claude Code, Cursor, OpenHands, etc.) and audits every file they touch. When the agent goes out of scope, you know immediately — and can approve, reject, or revert.

## The Problem

You tell your AI coding agent: "fix the retry timeout bug in payments." Twenty minutes later, the agent has fixed the bug — but also refactored subscriptions, changed 3 config files, added a new dependency, and left 2 TODOs in production code.

The scope was 1 file. The agent touched 12.

## The Solution

```bash
# Define scope before work
scopesafe init --task="fix retry timeout in payments" \
  --files="payments/*.go" \
  --exclude="payments/subscriptions/*,payments/vendor/*"

# Agent calls this automatically when it touches a file
scopesafe track --file=payments/retry.go --action=modify

# After work, audit what happened
scopesafe audit
```

Output:
```
═══ SCOPE AUDIT ═══
Task: fix retry timeout in payments
Duration: 23 minutes

FILES IN SCOPE (3):
  ✓ payments/retry.go — modified
  ✓ payments/timeout_test.go — modified
  ✓ payments/config.go — modified

FILES OUT OF SCOPE (2):
  ⚠ payments/subscriptions/main.go — modified [REQUIRES APPROVAL]
  ⚠ .env — modified [BLOCKED — secrets detected]

SCOPE SCORE: 68% (3/5 files in scope)

DECISION REQUIRED:
  - payments/subscriptions/main.go: approve or revert?
  - .env: BLOCKED (secrets cannot be modified)
```

## Features

| Feature | Description |
|---------|-------------|
| **Scope Definition** | Define task + files + exclusions before work |
| **File Tracking** | Log every create/modify/delete the agent performs |
| **Auto-Block Secrets** | `.env`, `*.pem`, `*.key`, `credentials.json` are automatically blocked |
| **Scope Audit** | End-of-task report with scope score, diff, approve/reject workflow |
| **Out-of-Scope Revert** | `scopesafe revert --all-out-of-scope` to undo non-approved changes |
| **MCP Server** | Connect directly to Claude Code, Cursor, OpenHands via MCP protocol |
| **Weekly Drift Report** | Track which agents tend to go out of scope and on which file types |

## Installation

```bash
# From source (requires Rust)
cargo install scopesafe

# Or download from GitHub releases
curl -fsSL https://github.com/dablon/scopesafe/releases/latest | sh
```

## Quick Start

```bash
# 1. Initialize a scope
scopesafe init --task="fix auth bug" --files="auth/*.go"

# 2. Agent works — each file modification is tracked
scopesafe track --file=auth/session.go --action=modify

# 3. Audit at the end
scopesafe audit

# 4. Approve or reject out-of-scope changes
scopesafe approve --file=auth/middleware.go
# OR
scopesafe reject --file=auth/middleware.go --reason="not in scope"
# OR revert all
scopesafe revert --all-out-of-scope
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `scopesafe init --task="..." --files="..." --exclude="..."` | Create a new scope |
| `scopesafe track --file=X --action=Y` | Log a file operation |
| `scopesafe audit` | Generate scope audit report |
| `scopesafe status` | Show current scope status |
| `scopesafe approve --file=X` | Approve an out-of-scope change |
| `scopesafe reject --file=X --reason="..."` | Reject with reason |
| `scopesafe revert --all-out-of-scope` | Revert all out-of-scope changes |
| `scopesafe report --weekly` | Weekly scope drift analysis |
| `scopesafe mcp` | Run as MCP server (v1.0) |

## MCP Integration

Add to your Claude Code `mcp.json`:

```json
{
  "mcpServers": {
    "scopesafe": {
      "command": "scopesafe",
      "args": ["mcp", "--project=/path/to/repo"]
    }
  }
}
```

The MCP server intercepts file operations and automatically tracks them against the active scope.

## Auto-Blocked Files

These files are automatically blocked from modification:

- `.env`, `.env.*`, `*.env`
- `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.jks`
- `credentials.json`, `secrets.json`, `secrets.yaml`
- `service-account*.json`
- `*.token`, `id_rsa*`, `id_ed25519*`, `id_ecdsa*`

## Architecture

```
src/
  cli.rs      — CLI interface (subcommands via clap)
  scope.rs    — Scope definition, file matching, blocked patterns
  tracker.rs  — File event tracking
  auditor.rs  — Audit report generation
  db.rs       — SQLite persistence
  mcp.rs      — MCP server (v1.0)
  patterns.rs — Scope drift analysis
  report.rs   — Weekly/monthly reports
```

Stack: **Rust** + **SQLite** (local, no cloud, privacy-first)

## Status

- **v0.1**: MVP — scope init, track, audit, approve/reject, auto-block secrets
- **v1.0**: MCP server, weekly drift reports, pattern analysis
- **v2.0**: CI integration, team collaboration, agent comparison dashboard

## License

MIT — Nicolas Alcaraz (@dablon)