#!/usr/bin/env node
// Downloads the prebuilt mcp-prune binary for this platform from the matching
// GitHub Release and writes it next to the Node shim in ./bin/.
//
// Fails loudly with a clear message on unsupported platforms or network errors.
// The shim in bin/mcp-prune.js is the npm entry point and will report a
// friendlier error if the binary is missing at run time, so a postinstall
// failure here doesn't break the install — it just defers the error.

const crypto = require("node:crypto");
const fs = require("node:fs");
const https = require("node:https");
const path = require("node:path");
const zlib = require("node:zlib");

const CHECKSUM_LINE_PATTERN = /\r?\n/;
const CHECKSUM_FIELD_PATTERN = /\s+/;
const LEADING_ASTERISK_PATTERN = /^\*/;

const REPO = "mstuart/mcp-prune";
const VERSION = require("./package.json").version;

const TARGETS = {
  "darwin-arm64": "aarch64-apple-darwin",
  "darwin-x64": "x86_64-apple-darwin",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "linux-x64": "x86_64-unknown-linux-gnu",
};

function targetTriple(platform = process.platform, arch = process.arch) {
  const key = `${platform}-${arch}`;
  const triple = TARGETS[key];
  if (!triple) {
    const supported = Object.keys(TARGETS).join(", ");
    throw new Error(
      `unsupported platform: ${key}. Supported: ${supported}. ` +
        `Build from source with \`cargo install --git https://github.com/${REPO}\`.`
    );
  }
  return triple;
}

function releaseUrl(triple, suffix = ".gz") {
  return `https://github.com/${REPO}/releases/download/v${VERSION}/mcp-prune-${triple}${suffix}`;
}

function outputPath(
  outDir = path.join(path.dirname(module.filename), "bin"),
  platform = process.platform
) {
  return path.join(
    outDir,
    platform === "win32" ? "mcp-prune-bin.exe" : "mcp-prune-bin"
  );
}

function parseChecksum(text, assetName) {
  for (const line of text.split(CHECKSUM_LINE_PATTERN)) {
    const fields = line.trim().split(CHECKSUM_FIELD_PATTERN);
    if (
      fields.length >= 2 &&
      fields[1].replace(LEADING_ASTERISK_PATTERN, "") === assetName
    ) {
      return fields[0].toLowerCase();
    }
  }
  throw new Error(`checksum for ${assetName} not found`);
}

function sha256(buffer) {
  return crypto.createHash("sha256").update(buffer).digest("hex");
}

function verifyChecksum(buffer, expected) {
  const actual = sha256(buffer);
  if (actual !== expected.toLowerCase()) {
    throw new Error(
      `checksum mismatch: expected ${expected.toLowerCase()}, got ${actual}`
    );
  }
}

function get(url, redirects = 5) {
  return new Promise((resolve, reject) => {
    https
      .get(
        url,
        { headers: { "user-agent": `mcp-prune-npm/${VERSION}` } },
        (res) => {
          if (
            res.statusCode >= 300 &&
            res.statusCode < 400 &&
            res.headers.location
          ) {
            if (redirects === 0) {
              reject(new Error("too many redirects"));
              return;
            }
            res.resume();
            resolve(get(res.headers.location, redirects - 1));
            return;
          }
          if (res.statusCode !== 200) {
            reject(new Error(`GET ${url} returned ${res.statusCode}`));
            return;
          }
          resolve(res);
        }
      )
      .on("error", reject);
  });
}

async function readStream(stream) {
  const chunks = [];
  for await (const chunk of stream) {
    chunks.push(Buffer.from(chunk));
  }
  return Buffer.concat(chunks);
}

async function downloadAndInstall({
  fetch = get,
  outDir = path.join(path.dirname(module.filename), "bin"),
  platform = process.platform,
  arch = process.arch,
} = {}) {
  const triple = targetTriple(platform, arch);
  const assetName = `mcp-prune-${triple}.gz`;
  const url = releaseUrl(triple);
  const checksumUrl = releaseUrl(triple, ".gz.sha256");
  const outPath = outputPath(outDir, platform);

  fs.mkdirSync(outDir, { recursive: true });

  process.stdout.write(`mcp-prune: downloading ${triple} binary... `);
  const [checksumStream, assetStream] = await Promise.all([
    fetch(checksumUrl),
    fetch(url),
  ]);
  const checksumText = (await readStream(checksumStream)).toString("utf8");
  const expected = parseChecksum(checksumText, assetName);
  const archive = await readStream(assetStream);
  verifyChecksum(archive, expected);
  fs.writeFileSync(outPath, zlib.gunzipSync(archive), { mode: 0o755 });
  fs.chmodSync(outPath, 0o755);
  process.stdout.write("done\n");
  return outPath;
}

async function main() {
  await downloadAndInstall();
}

if (require.main === module) {
  main().catch((err) => {
    console.error(`mcp-prune: postinstall failed: ${err.message}`);
    console.error(
      "mcp-prune: re-run `npm rebuild mcp-prune` to retry the GitHub Releases download."
    );
    process.exit(0);
  });
}

module.exports = {
  downloadAndInstall,
  outputPath,
  parseChecksum,
  releaseUrl,
  sha256,
  TARGETS,
  targetTriple,
  verifyChecksum,
};
