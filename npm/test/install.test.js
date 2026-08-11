const assert = require("node:assert/strict");
const { Readable } = require("node:stream");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const zlib = require("node:zlib");

const {
  downloadAndInstall,
  parseChecksum,
  sha256,
  targetTriple,
  verifyChecksum,
} = require("../install");

const UNSUPPORTED_PLATFORM_PATTERN = /unsupported platform: freebsd-x64/;
const CHECKSUM_MISMATCH_PATTERN = /checksum mismatch/;

test("targetTriple maps supported npm platform targets", () => {
  assert.equal(targetTriple("darwin", "arm64"), "aarch64-apple-darwin");
  assert.equal(targetTriple("darwin", "x64"), "x86_64-apple-darwin");
  assert.equal(targetTriple("linux", "x64"), "x86_64-unknown-linux-gnu");
  assert.equal(targetTriple("linux", "arm64"), "aarch64-unknown-linux-gnu");
});

test("targetTriple rejects unsupported targets with supported list", () => {
  assert.throws(
    () => targetTriple("freebsd", "x64"),
    UNSUPPORTED_PLATFORM_PATTERN
  );
});

test("parseChecksum accepts shasum output for the requested asset", () => {
  assert.equal(
    parseChecksum("abcd  mcp-prune-x.gz\n", "mcp-prune-x.gz"),
    "abcd"
  );
  assert.equal(
    parseChecksum("abcd *mcp-prune-x.gz\n", "mcp-prune-x.gz"),
    "abcd"
  );
});

test("verifyChecksum detects tampered downloads", () => {
  const archive = Buffer.from("downloaded asset");
  verifyChecksum(archive, sha256(archive));
  assert.throws(
    () => verifyChecksum(archive, "0".repeat(64)),
    CHECKSUM_MISMATCH_PATTERN
  );
});

test("downloadAndInstall verifies checksum before writing native binary", async () => {
  const outDir = fs.mkdtempSync(path.join(os.tmpdir(), "mcp-prune-install-"));
  const binary = Buffer.from("#!/bin/sh\necho mcp-prune test\n");
  const archive = zlib.gzipSync(binary);
  const checksum = `${sha256(archive)}  mcp-prune-x86_64-unknown-linux-gnu.gz\n`;
  const requested = [];
  const fetch = (url) => {
    requested.push(url);
    if (url.endsWith(".sha256")) {
      return Readable.from([checksum]);
    }
    return Readable.from([archive]);
  };

  const outPath = await downloadAndInstall({
    arch: "x64",
    fetch,
    outDir,
    platform: "linux",
  });

  assert.deepEqual(
    requested.map((url) => path.basename(url)),
    [
      "mcp-prune-x86_64-unknown-linux-gnu.gz.sha256",
      "mcp-prune-x86_64-unknown-linux-gnu.gz",
    ]
  );
  assert.equal(fs.readFileSync(outPath, "utf8"), binary.toString("utf8"));
  fs.accessSync(outPath, fs.constants.X_OK);
});

test("downloadAndInstall fails before writing when checksum mismatches", async () => {
  const outDir = fs.mkdtempSync(
    path.join(os.tmpdir(), "mcp-prune-install-bad-")
  );
  const archive = zlib.gzipSync(Buffer.from("binary"));
  const fetch = (url) => {
    if (url.endsWith(".sha256")) {
      return Readable.from([
        `${"0".repeat(64)}  mcp-prune-x86_64-unknown-linux-gnu.gz\n`,
      ]);
    }
    return Readable.from([archive]);
  };

  await assert.rejects(
    () => downloadAndInstall({ arch: "x64", fetch, outDir, platform: "linux" }),
    CHECKSUM_MISMATCH_PATTERN
  );
  assert.equal(fs.existsSync(path.join(outDir, "mcp-prune-bin")), false);
});
