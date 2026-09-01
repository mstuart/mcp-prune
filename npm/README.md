# mcp-prune

Audit MCP server usage from Claude Code transcripts. Find idle servers so you
can prune them and stop loading their tool schemas into every conversation.

This npm package wraps the Rust binary distributed via GitHub Releases.

Requires Node.js 16 or newer. Supported prebuilt targets are macOS (Apple
silicon and Intel) and glibc-based Linux (x64 and ARM64). Linux systems using
musl, including Alpine, require a source build.

## Install

```sh
npm install -g mcp-prune
# or run without installing
npx mcp-prune report
```

On install, a small postinstall script downloads the prebuilt binary for your
platform (`darwin-arm64`, `darwin-x64`, `linux-x64`, `linux-arm64`) from the
matching GitHub Release. No Rust toolchain required.

## Usage

```sh
mcp-prune install          # add SessionStart hook to ~/.claude/settings.json
mcp-prune report           # grouped usage report
mcp-prune idle             # only idle servers
mcp-prune apply            # interactive prune
```

See the full README at https://github.com/mstuart/mcp-prune for details.

## Troubleshooting

If the binary fails to download during install (corp proxy, offline, etc.):

```sh
npm rebuild mcp-prune        # retry the download
```

Or build from source:

```sh
cargo install --git https://github.com/mstuart/mcp-prune
```

## License

MIT
