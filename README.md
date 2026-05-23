# mcp-prune

[![CI](https://github.com/mstuart/mcp-prune/actions/workflows/ci.yml/badge.svg)](https://github.com/mstuart/mcp-prune/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Audit MCP server usage from Claude Code transcripts. Find idle servers so you
can prune them and stop loading their tool schemas into every conversation.

Every MCP server you keep configured but don't actually use is dead weight —
its tool definitions ship into your context on every session, costing tokens
and crowding the model's tool-selection space whether you ever call the
server or not. `mcp-prune` parses `~/.claude/projects/**/*.jsonl` in parallel,
counts real `tool_use` events per server, and tells you which ones have been
idle long enough to disable.

## Install

```sh
npm install -g mcp-prune
# or run without installing
npx mcp-prune report

mcp-prune install   # appends a SessionStart hook to ~/.claude/settings.json
```

The npm package downloads a prebuilt binary for your platform on install — no
Rust toolchain required. Supported: `darwin-arm64`, `darwin-x64`, `linux-x64`,
`linux-arm64`.

The hook installer creates a timestamped backup of `settings.json` before
writing.

### From source

```sh
cargo install --git https://github.com/mstuart/mcp-prune
# or, in a clone
cargo install --path .
```

## Usage

```sh
mcp-prune report           # grouped usage report — active / idle / never called
mcp-prune report --json    # same, machine-readable
mcp-prune report --fresh   # bypass the 24h cache, rescan transcripts
mcp-prune report --all     # include historical entries (in transcripts, not configured)
mcp-prune idle             # only idle servers (warn/alert/unused)
mcp-prune idle --json      # same, same envelope as `report --json`
mcp-prune apply            # interactive prune — prompts per idle server
mcp-prune apply --dry-run  # print what would be removed without doing it
mcp-prune apply -y         # auto-confirm; remove every removable idle server
mcp-prune install          # add the SessionStart hook to ~/.claude/settings.json
mcp-prune uninstall        # remove the SessionStart hook
mcp-prune config-show      # print resolved config
```

Sample:

```
mcp-prune  2107 transcripts · 2026-05-23 03:16 UTC
warn ≥7d  ·  alert ≥14d

  ●  active (3 servers)
     gcp-observability             5d   384 calls   66 in 7d  [project]
     plugin_claude-mem_mcp-search  0d     6 calls    6 in 7d  [plugin]
     vexp                          0d     4 calls    4 in 7d

  ○  idle ≥14d (4)
     gsd-workflow                  15d    94 calls   94 in 30d  [project]
     plugin_playwright_playwright  27d   374 calls   30 in 30d  [plugin]
     collectors-admin              17d     5 calls    5 in 30d  [project]
     bullmq                        28d    29 calls    1 in 30d  [project]

  ⌀  never called (3)
     atlassian                     —   0 calls   —  [project]
     statsig                       —   0 calls   —  [project]
     gitnexus                      —   0 calls   —  [project]

  → 4 servers idle · run `mcp-prune apply` to review and remove
    (19 stale entries hidden — pass --all to show)
```

Tags after each row show where the server is configured: `[project]` for a
per-directory entry in `~/.claude.json` or a `.mcp.json`, `[plugin]` for a
server provided by an installed plugin, no tag for a user-scope entry at
`~/.claude.json` root.

Status markers: `●` active · `◐` warn · `○` alert · `⌀` never called. Color
is auto-disabled when stdout isn't a terminal; set `NO_COLOR` to opt out
explicitly or `FORCE_COLOR` to override.

### Pruning

`mcp-prune apply` walks each idle server in order and prompts for
confirmation. On approval it shells out to `claude mcp remove <name>`.

MCP servers can be configured at user scope (`~/.claude.json` root), local
scope (per-directory entries inside `~/.claude.json`), or shared-project scope
(a committed `.mcp.json` in a project root). `claude mcp remove` only finds
local-scope entries when invoked from inside the owning directory, so `apply`
tracks the project dir per server and `cd`s into it before invoking. Plugin-
defined servers (`plugin_*`) are flagged but not auto-removed — those need
`claude plugin disable`, which has different scope semantics.

`--dry-run` shows the plan without executing; `-y` skips prompts for
everything that's safe to remove.

## How it classifies

| Report group   | Status   | Meaning                                                              |
|----------------|----------|----------------------------------------------------------------------|
| `active`       | `ok`     | Called within `warn_days` (default: 7)                               |
| `warn ≥Xd`     | `WARN`   | Idle ≥ `warn_days` but < `alert_days`                                |
| `idle ≥Xd`     | `ALERT`  | Idle ≥ `alert_days` (default: 14) — strong candidate to disable      |
| `never called` | `UNUSED` | Server is configured but has zero `tool_use` events in transcripts   |

`UNUSED` typically means the server's tools were loaded into context but never
chosen — the worst kind of dead weight.

## Configuration

Defaults work out of the box. To override, create
`~/.config/mcp-prune/config.toml`:

```toml
warn_days = 7
alert_days = 14
transcripts_dir = "/Users/you/.claude/projects"
cache_path = "/Users/you/.claude/cache/mcp-prune.json"
```

`alert_days` must be ≥ `warn_days`, both must be ≥ 0. Invalid configs are
rejected at load with a clear error.

## SessionStart hook

Once installed, the hook runs at the start of every Claude Code session and
refreshes the cache in the background (`{"continue":true,"suppressOutput":true}`
— silent, no UI noise). `mcp-prune report` reads from the cache and is
near-instant. Cache TTL is 24 hours.

## How it works

1. `walkdir` enumerates every `.jsonl` under the transcripts dir
2. `rayon` parses files in parallel — ~2000 transcripts in <1s on an M-series Mac
3. For each line containing `mcp__`:
   - Counts `tool_use` events as real calls (authoritative usage signal)
   - Reads `attachment.addedNames` and `attachment.addedLines` arrays to mark
     servers as `configured` (the actual MCP tool manifest Claude Code
     attached that turn)
   - Falls back to scanning line-prefix `mcp__` tokens in message text for
     servers that surface via system reminders
4. Aggregates per-server stats (7d / 14d / 30d / total) and last-call timestamp
5. Classifies each server against the warn/alert thresholds

Inline doc mentions like `` `mcp__servername__toolname` `` are deliberately
ignored — only attached tool names and line-start references count, so
discussions about the tool itself don't pollute the report.

## Privacy

`mcp-prune` runs entirely locally. It reads JSONL transcripts from your own
`~/.claude/projects/` directory, writes a JSON cache to
`~/.claude/cache/mcp-prune.json`, and prints to stdout. **No network calls, no
telemetry, no external services.** The tool never opens a socket. Source is
small and worth a skim if you want to verify (`src/scan.rs` is the only file
that touches transcript data).

## Compared to alternatives

[nnnkkk7/mcp-tidy](https://github.com/nnnkkk7/mcp-tidy) is the existing
Node.js tool in this space. It manages MCP server configuration (enable /
disable / install) but doesn't report usage data — you decide what to prune
based on memory, not measurements. `mcp-prune` is the inverse: it doesn't
edit your config, it just gives you the receipts for what's actually called
versus what's loaded. Pair them: `mcp-prune idle` to find the dead weight,
`mcp-tidy` (or `claude mcp remove`) to act on it.

## Why this exists

Claude Code's MCP system is great but easy to over-subscribe to: install five
plugins for one trial, forget to disable them, and you're paying tokens
forever. `mcp-prune` is the receipt — actual usage vs. configured presence.

## License

MIT
