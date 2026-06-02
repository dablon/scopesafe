# scopesafe — AI Agent Scope Guardrail

## 1. Concept & Vision

**Problem:** You tell your AI coding agent (Claude Code, Cursor, OpenHands, etc.): "fix the retry timeout bug in payments." The agent comes back 20 minutes later having fixed the bug, but also refactored subscriptions, changed 3 unrelated config files, added a new dependency, and left 2 TODOs in production code. The scope was 1 file. The agent touched 12.

**Solution:** scopesafe is the guardrail between intent and execution. Define the scope BEFORE work starts. While the agent works, scopesafe watches. At the end, scopesafe audits: "You touched these 12 files. Only 1 was in scope. Here's the diff. Approve or reject."

**Personality:** The responsible adult in the room. Not flashy. Not an AI itself. A security camera, not a participant. Quiet until something goes wrong.

**Tagline:** "Your agent has a scope. It should respect it."

## 2. Design Language

**Aesthetic:** Terminal-first. Monochrome. Signal over noise. Think `htop` not `figma`.

**Colors (ANSI):**
- Good: green (#00FF00 on dark) — within scope, approved
- Warning: yellow (#FFFF00) — touched outside scope, pending approval  
- Danger: red (#FF0000) — critical deviation, blocked
- Neutral: white/gray — informational

**Typography:** Monospace. Terminal only. No GUI for v1.0.

**Motion:** No animations. Instant output. This is a tool, not a toy.

## 3. Core Feature: Scope Definition + Audit

### 3.1 Before Work: Define Scope

```bash
scopesafe init --task="fix retry timeout in payments"
# OR with file-level granularity
scopesafe init --task="fix retry timeout" --files="payments/*.go" --exclude="payments/subscriptions/*"
```

Outputs a `SCOPE.md` in the repo:
```markdown
# Scope: fix retry timeout in payments
- files: payments/*.go
- exclude: payments/subscriptions/*, payments/vendor/*
- created: 2026-06-02T03:00:00Z
- owner: nico
```

### 3.2 During Work: Track Agent Actions

The agent writes to a scope log:
```bash
scopesafe track --file=payments/retry.go --action=modify
scopesafe track --file=payments/subscriptions/main.go --action=modify
```

### 3.3 After Work: Audit Report

```bash
scopesafe audit
```

Output:
```
=== SCOPE AUDIT ===
Task: fix retry timeout in payments
Duration: 23 minutes

FILES IN SCOPE (3):
  ✓ payments/retry.go — modified
  ✓ payments/timeout_test.go — modified  
  ✓ payments/config.go — modified

FILES OUT OF SCOPE (2):
  ⚠ payments/subscriptions/main.go — modified [REQUIRES APPROVAL]
  ⚠ .env — modified [BLOCKED — secret files cannot be modified]

CHANGES SUMMARY:
  In-scope: +42 -12 lines
  Out-of-scope: +89 -23 lines

SCOPE SCORE: 68% (3/5 files in scope)

DECISION REQUIRED:
  - payments/subscriptions/main.go: approve or revert?
  - .env: BLOCKED (secrets detected)

Run: scopesafe approve --file=payments/subscriptions/main.go
     scopesafe reject --file=payments/subscriptions/main.go --reason="not in scope"
     scopesafe revert --all-out-of-scope
```

## 4. MCP Server Integration

scopesafe runs as an MCP server that AI coding agents can connect to via MCP protocol:

```json
// Claude Code's mcp.json
{
  "mcpServers": {
    "scopesafe": {
      "command": "scopesafe",
      "args": ["mcp", "--project=/path/to/repo"]
    }
  }
}
```

When the agent runs a tool that modifies files, scopesafe intercepts and logs:
- File touched
- Action (create/modify/delete)
- Timestamp
- Backing it up before change

## 5. CLI Commands

| Command | Description |
|---------|-------------|
| `scopesafe init --task="..." --files="..."` | Initialize scope for a task |
| `scopesafe track --file=X --action=Y` | Log a file operation |
| `scopesafe audit` | Generate scope audit report |
| `scopesafe approve --file=X` | Approve out-of-scope change |
| `scopesafe reject --file=X --reason="..."` | Reject with reason |
| `scopesafe revert --all-out-of-scope` | Revert all out-of-scope changes |
| `scopesafe status` | Show current scope status |
| `scopesafe mcp` | Run as MCP server |
| `scopesafe agent --claude-code` | Agent-mode: attach to Claude Code session |
| `scopesafe report --weekly` | Weekly scope drift report |

## 6. Scope Drift Detection

### 6.1 Pattern Detection

After running for a while, scopesafe learns:
- "Your agent tends to expand scope when working on auth-related files"
- "You consistently reject changes to .env"
- "The agent ignores --exclude patterns 40% of the time in Cursor sessions"

### 6.2 Weekly Drift Report

```bash
scopesafe report --weekly
```

Output:
```
=== SCOPE DRIFT REPORT — Week of May 26-Jun 2 ===

Total tasks tracked: 14
Avg scope score: 71%

SCOPE VIOLATIONS:
  - 3 tasks: touched files not in original scope
  - 2 tasks: modified .env files
  - 1 task: added new dependencies without approval

TOP EXPANSION PATTERNS:
  1. "refactor adjacent code" (8 occurrences)
  2. "add logging" (5 occurrences)
  3. "fix typo in comment" (4 occurrences)

AGENT COMPARISON:
  Cursor:  68% avg scope score
  Claude Code: 74% avg scope score
  OpenHands: 61% avg scope score

RECOMMENDATIONS:
  - Add '*.env' to global exclude list
  - Add prompt to Claude Code: "Do not modify files outside scope"
  - Review Cursor agent config: disable auto-import
```

## 7. Technical Architecture

### Stack
- **Language:** Rust (fast, low overhead, single binary)
- **Persistence:** SQLite (local, no cloud, privacy-first)
- **MCP:** Native MCP server implementation
- **Agent integrations:** Claude Code, Cursor, OpenHands, Continue, Copilot

### Modules

```
src/
  cli.rs          — CLI interface (subcommands)
  scope.rs        — Scope definition and parsing
  tracker.rs      — File operation tracking
  auditor.rs      — Post-work audit report generation
  mcp.rs          — MCP server implementation
  agent.rs        — Agent session attachment
  patterns.rs     — Drift pattern analysis
  report.rs       — Weekly/monthly reports
  db.rs           — SQLite persistence
  main.rs         — Entry point, command dispatch
```

### Data Model

**Scope:**
```
id, task, files_pattern, exclude_pattern, created_at, owner, status
```

**FileEvent:**
```
id, scope_id, file_path, action, timestamp, approved, approved_by, in_scope
```

**WeeklyReport:**
```
id, week_start, total_tasks, avg_score, top_violations, agent_breakdown
```

## 8. Privacy & Security

- **All data is local.** No cloud. No telemetry. No account required.
- **Secrets detection:** .env, *.key, *.pem, credentials.json are automatically BLOCKED from modification
- **Git integration:** All changes are tracked, never destructive unless explicitly approved
- **Audit log is append-only.** You can always see what happened.

## 9. Competitive Differentiation

| Feature | scopesafe | agent-audit | Claude Code hooks |
|---------|-----------|-------------|-------------------|
| Scope definition | ✅ | ❌ | ❌ |
| Out-of-scope detection | ✅ | ❌ | ❌ |
| Auto-block secrets | ✅ | ❌ | ❌ |
| Agent-agnostic | ✅ | ❌ | ❌ |
| Scope drift report | ✅ | ❌ | ❌ |
| MCP server | ✅ | ❌ | ❌ |
| Approve/reject workflow | ✅ | ❌ | ❌ |

## 10. MVP Scope (v0.1)

**Must have:**
- `scopesafe init` with task + file patterns
- `scopesafe track` to log file operations
- `scopesafe audit` to generate report
- `scopesafe status` to show current scope
- `scopesafe revert --all-out-of-scope`
- SQLite persistence
- `.env`, `*.key`, `*.pem` auto-block

**v1.0:**
- MCP server
- Agent session attachment
- Weekly drift report
- Pattern detection

**v2.0:**
- Agent comparison dashboard
- CI integration
- Team collaboration (shared scope history)
