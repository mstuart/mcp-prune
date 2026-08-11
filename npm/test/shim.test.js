const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const { nativeBinaryPath, run } = require("../bin/mcp-prune");

const MISSING_BINARY_PATTERN = /native binary not found/;
const NPM_REBUILD_PATTERN = /npm rebuild mcp-prune/;

test("nativeBinaryPath selects platform-specific binary names", () => {
  assert.equal(
    nativeBinaryPath("linux", "/pkg/bin"),
    path.join("/pkg/bin", "mcp-prune-bin")
  );
  assert.equal(
    nativeBinaryPath("win32", "/pkg/bin"),
    path.join("/pkg/bin", "mcp-prune-bin.exe")
  );
});

test("shim reports missing native binary", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "mcp-prune-shim-missing-"));
  const originalError = console.error;
  const lines = [];
  console.error = (...args) => lines.push(args.join(" "));
  try {
    assert.equal(run(["--version"], { dir, platform: "linux" }), 1);
  } finally {
    console.error = originalError;
  }
  assert.match(lines.join("\n"), MISSING_BINARY_PATTERN);
  assert.match(lines.join("\n"), NPM_REBUILD_PATTERN);
});

test("shim forwards arguments to the native binary", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "mcp-prune-shim-run-"));
  fs.writeFileSync(path.join(dir, "mcp-prune-bin"), "not used");
  const calls = [];
  const status = run(["--version"], {
    dir,
    platform: "linux",
    spawn: (binPath, argv, opts) => {
      calls.push({ argv, binPath, opts });
      return { status: 0 };
    },
  });

  assert.equal(status, 0);
  assert.equal(calls[0].binPath, path.join(dir, "mcp-prune-bin"));
  assert.deepEqual(calls[0].argv, ["--version"]);
  assert.deepEqual(calls[0].opts, { stdio: "inherit" });
});
