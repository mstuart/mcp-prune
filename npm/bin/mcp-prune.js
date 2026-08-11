#!/usr/bin/env node
// Thin shim that exec's the platform-native binary that postinstall placed
// next to this file. Keeping this as JS (not the binary itself) means npm can
// always link a stable `bin/mcp-prune.js` regardless of host arch.

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

function nativeBinaryPath(
  platform = process.platform,
  dir = path.dirname(module.filename)
) {
  const ext = platform === "win32" ? ".exe" : "";
  return path.join(dir, `mcp-prune-bin${ext}`);
}

function run(
  argv = process.argv.slice(2),
  {
    platform = process.platform,
    dir = path.dirname(module.filename),
    spawn = spawnSync,
  } = {}
) {
  const binPath = nativeBinaryPath(platform, dir);

  if (!fs.existsSync(binPath)) {
    console.error("mcp-prune: native binary not found at", binPath);
    console.error(
      "mcp-prune: re-run install with `npm rebuild mcp-prune` (postinstall fetches it from GitHub Releases)."
    );
    console.error(
      "mcp-prune: if this keeps failing, report at https://github.com/mstuart/mcp-prune/issues"
    );
    return 1;
  }

  const result = spawn(binPath, argv, { stdio: "inherit" });
  if (result.error) {
    console.error("mcp-prune:", result.error.message);
    return 1;
  }
  return result.status ?? 1;
}

if (require.main === module) {
  process.exit(run());
}

module.exports = { nativeBinaryPath, run };
