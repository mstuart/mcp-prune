# mcp-pulse

Audit MCP server usage from Claude Code transcripts. Find idle servers so you
can prune them and stop loading their tool schemas into every conversation.

Every MCP server you keep configured but don't actually use is dead weight —
its tool definitions ship into your context on every session, even with
`ENABLE_TOOL_SEARCH=true`. `mcp-pulse` parses `~/.claude/projects/**/*.jsonl`
in parallel, counts real `tool_use` events per server, and tells you which
ones have been idle long enough to disable.

## Install

```sh
cargo install --path .
mcp-pulse install   # appends a SessionStart hook to ~/.claude/settings.json
```

The installer creates a timestamped backup of `settings.json` before writing.

## Usage

```sh
mcp-pulse report          # full table — all servers, sorted by 30d calls
mcp-pulse report --json   # same, machine-readable
mcp-pulse report --fresh  # bypass the 24h cache, rescan transcripts
mcp-pulse idle            # only servers in warn/alert/unused state
mcp-pulse config-show     # print resolved config
```

Sample:

```
MCP Pulse — scanned 1975 transcripts at 2026-05-21 10:41 UTC
Thresholds: warn ≥7d  alert ≥14d

server                            7d     14d     30d    total    last (d)  status
----------------------------------------------------------------------------------------
gcp-observability                 66      66     345      384           3  ok
gsd-workflow                       0       0      94       94          14  ALERT
plugin_playwright_playwright       0       0      30      374          25  ALERT
github                             0       0       6        9          18  ALERT
vexp                               0       0       0        0           —  UNUSED
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
`~/.config/mcp-pulse/config.toml`:

```toml
warn_days = 7
alert_days = 14
scan_window_days = 30
transcripts_dir = "/Users/you/.claude/projects"
cache_path = "/Users/you/.claude/cache/mcp-pulse.json"
```

## SessionStart hook

Once installed, the hook runs at the start of every Claude Code session and
refreshes the cache in the background (`{"continue":true,"suppressOutput":true}`
— silent, no UI noise). `mcp-pulse report` reads from the cache and is
near-instant. Cache TTL is 24 hours.

## How it works

1. `walkdir` enumerates every `.jsonl` under the transcripts dir
2. `rayon` parses files in parallel — ~1975 transcripts in 0.8s on an M-series Mac
3. For each line containing `mcp__`:
   - Counts `tool_use` events as real calls
   - Tracks server appearances in deferred-tool listings as `configured = true`
4. Aggregates per-server stats (7d / 14d / 30d / total) and last-call timestamp
5. Classifies each server against the warn/alert thresholds

## Why this exists

Claude Code's MCP system is great but easy to over-subscribe to: install five
plugins for one trial, forget to disable them, and you're paying tokens
forever. `mcp-pulse` is the receipt — actual usage vs. configured presence.

## License

MIT
