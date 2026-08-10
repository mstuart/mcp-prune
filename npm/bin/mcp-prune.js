#!/usr/bin/env node
// Thin shim that exec's the platform-native binary that postinstall placed
// next to this file. Keeping this as JS (not the binary itself) means npm can
// always link a stable `bin/mcp-prune.js` regardless of host arch.

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const ext = process.platform === "win32" ? ".exe" : "";
const binPath = path.join(import.meta.dirname, `mcp-prune-bin${ext}`);

if (!fs.existsSync(binPath)) {
  console.error("mcp-prune: native binary not found at", binPath);
  console.error(
    "mcp-prune: re-run install with `npm rebuild mcp-prune` (postinstall fetches it from GitHub Releases)."
  );
  console.error(
    "mcp-prune: if this keeps failing, report at https://github.com/mstuart/mcp-prune/issues"
  );
  process.exit(1);
}

const result = spawnSync(binPath, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error("mcp-prune:", result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
