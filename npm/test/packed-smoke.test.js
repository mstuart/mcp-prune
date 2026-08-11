const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const test = require("node:test");

const VERSION_PATTERN = /^mcp-prune \d+\.\d+\.\d+/;

const repoRoot = path.resolve(path.dirname(module.filename), "..", "..");
const npmRoot = path.join(repoRoot, "npm");

function run(cmd, args, opts = {}) {
  const result = spawnSync(cmd, args, { encoding: "utf8", ...opts });
  if (result.status !== 0) {
    throw new Error(
      `${cmd} ${args.join(" ")} failed\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`
    );
  }
  return result;
}

function builtBinary() {
  const release = path.join(repoRoot, "target", "release", "mcp-prune");
  const debug = path.join(repoRoot, "target", "debug", "mcp-prune");
  if (fs.existsSync(release)) {
    return release;
  }
  if (fs.existsSync(debug)) {
    return debug;
  }
  run("cargo", ["build", "--release"], { cwd: repoRoot });
  return release;
}

test("packed npm package installs and shim runs --version", () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "mcp-prune-pack-"));
  const pack = run("npm", ["pack", "--json", "--pack-destination", temp], {
    cwd: npmRoot,
  });
  const tarball = path.join(temp, JSON.parse(pack.stdout)[0].filename);
  run("npm", ["install", "--ignore-scripts", tarball], { cwd: temp });

  const installedBinDir = path.join(temp, "node_modules", "mcp-prune", "bin");
  fs.copyFileSync(builtBinary(), path.join(installedBinDir, "mcp-prune-bin"));
  fs.chmodSync(path.join(installedBinDir, "mcp-prune-bin"), 0o755);

  const smoke = run(
    path.join(temp, "node_modules", ".bin", "mcp-prune"),
    ["--version"],
    { cwd: temp }
  );
  assert.match(smoke.stdout, VERSION_PATTERN);
});
