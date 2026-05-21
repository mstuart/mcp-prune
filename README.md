# mcp-prune

[![CI](https://github.com/mstuart/mcp-prune/actions/workflows/ci.yml/badge.svg)](https://github.com/mstuart/mcp-prune/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Audit MCP server usage from Claude Code transcripts. Find idle servers so you
can prune them and stop loading their tool schemas into every conversation.

Every MCP server you keep configured but don't actually use is dead weight —
its tool definitions ship into your context on every session, even with
`ENABLE_TOOL_SEARCH=true`. `mcp-prune` parses `~/.claude/projects/**/*.jsonl`
in parallel, counts real `tool_use` events per server, and tells you which
ones have been idle long enough to disable.

## Install

```sh
cargo install --path .
mcp-prune install   # appends a SessionStart hook to ~/.claude/settings.json
```

The installer creates a timestamped backup of `settings.json` before writing.

## Usage

```sh
mcp-prune report          # full table — all servers, sorted by 30d calls
mcp-prune report --json   # same, machine-readable
mcp-prune report --fresh  # bypass the 24h cache, rescan transcripts
mcp-prune idle            # only servers in warn/alert/unused state
mcp-prune config-show     # print resolved config
mcp-prune uninstall       # remove the SessionStart hook
```

Sample:

```
mcp-prune — scanned 1990 transcripts at 2026-05-21 11:38 UTC
Thresholds: warn ≥7d  alert ≥14d

server                                    7d     14d     30d    total    last (d)  status
------------------------------------------------------------------------------------------------
gcp-observability                         66      66     345      384           3  ok
gsd-workflow                               0       0      94       94          14  ALERT
grove                                     35      35      35       35           3  ok
plugin_playwright_playwright               0       0      30      374          25  ALERT
github                                     0       0       6        9          18  ALERT
plugin_context7_context7                   2       2       2       15           0  ok
bullmq                                     0       0       1       29          27  ALERT
plugin_slack_slack                         0       0       0        0           —  UNUSED
plugin_vercel_vercel                       0       0       0        0           —  UNUSED
vexp                                       0       0       0        0           —  UNUSED
```

## How it classifies

| Status   | Meaning                                                                |
|----------|------------------------------------------------------------------------|
| `ok`     | Called within `warn_days` (default: 7)                                 |
| `WARN`   | Idle ≥ `warn_days` but < `alert_days`                                  |
| `ALERT`  | Idle ≥ `alert_days` (default: 14) — strong candidate to disable        |
| `UNUSED` | Server appears in transcript metadata but has zero `tool_use` events   |

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
