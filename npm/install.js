#!/usr/bin/env node
// Downloads the prebuilt mcp-prune binary for this platform from the matching
// GitHub Release and writes it next to the Node shim in ./bin/.
//
// Fails loudly with a clear message on unsupported platforms or network errors.
// The shim in bin/mcp-prune.js is the npm entry point and will report a
// friendlier error if the binary is missing at run time, so a postinstall
// failure here doesn't break the install — it just defers the error.

'use strict';

const fs = require('fs');
const path = require('path');
const https = require('https');
const zlib = require('zlib');
const { pipeline } = require('stream/promises');

const REPO = 'mstuart/mcp-prune';
const VERSION = require('./package.json').version;

const TARGETS = {
  'darwin-arm64': 'aarch64-apple-darwin',
  'darwin-x64': 'x86_64-apple-darwin',
  'linux-x64': 'x86_64-unknown-linux-gnu',
  'linux-arm64': 'aarch64-unknown-linux-gnu',
};

function targetTriple() {
  const key = `${process.platform}-${process.arch}`;
  const triple = TARGETS[key];
  if (!triple) {
    const supported = Object.keys(TARGETS).join(', ');
    throw new Error(
      `unsupported platform: ${key}. Supported: ${supported}. ` +
        `Build from source with \`cargo install --git https://github.com/${REPO}\`.`
    );
  }
  return triple;
}

function get(url, redirects = 5) {
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { 'user-agent': `mcp-prune-npm/${VERSION}` } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          if (redirects === 0) {
            reject(new Error('too many redirects'));
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
      })
      .on('error', reject);
  });
}

async function main() {
  const triple = targetTriple();
  const url = `https://github.com/${REPO}/releases/download/v${VERSION}/mcp-prune-${triple}.gz`;
  const outDir = path.join(__dirname, 'bin');
  const outPath = path.join(outDir, process.platform === 'win32' ? 'mcp-prune-bin.exe' : 'mcp-prune-bin');

  fs.mkdirSync(outDir, { recursive: true });

  process.stdout.write(`mcp-prune: downloading ${triple} binary... `);
  const res = await get(url);
  await pipeline(res, zlib.createGunzip(), fs.createWriteStream(outPath, { mode: 0o755 }));
  process.stdout.write('done\n');
}

main().catch((err) => {
  console.error(`mcp-prune: postinstall failed: ${err.message}`);
  console.error(`mcp-prune: the binary will be downloaded on first run, or you can re-run \`npm rebuild mcp-prune\`.`);
  process.exit(0);
});
