"use strict";

// Downloads the release binary matching this platform. Runs on postinstall.
//
// Failure here must not abort the install: `npm install` aborting on a network
// blip is worse than a clear message at first use, and the launcher already
// reports a missing binary with instructions.

const fs = require("node:fs");
const https = require("node:https");
const path = require("node:path");

const pkg = require("../package.json");
const { targetTriple, binaryName, binaryPath } = require("./platform");

const REPO = "Sarthak-47/Dross";

function assetUrl(triple) {
  const suffix = triple.includes("windows") ? ".exe" : "";
  return `https://github.com/${REPO}/releases/download/v${pkg.version}/dross-${triple}${suffix}`;
}

function download(url, dest, redirectsLeft = 5) {
  return new Promise((resolve, reject) => {
    https
      .get(url, (response) => {
        const { statusCode, headers } = response;

        if (statusCode >= 300 && statusCode < 400 && headers.location) {
          response.resume();
          if (redirectsLeft === 0) {
            reject(new Error("too many redirects"));
            return;
          }
          resolve(download(headers.location, dest, redirectsLeft - 1));
          return;
        }

        if (statusCode !== 200) {
          response.resume();
          reject(new Error(`HTTP ${statusCode} for ${url}`));
          return;
        }

        fs.mkdirSync(path.dirname(dest), { recursive: true });
        const file = fs.createWriteStream(dest);
        response.pipe(file);
        file.on("finish", () => file.close(() => resolve()));
        file.on("error", reject);
      })
      .on("error", reject);
  });
}

async function main() {
  const triple = targetTriple();
  if (!triple) {
    console.warn(
      `[dross] no prebuilt binary for ${process.platform}/${process.arch}. ` +
        "Build from source: cargo build --release -p dross-cli",
    );
    return;
  }

  const dest = binaryPath();
  if (fs.existsSync(dest)) {
    return;
  }

  const url = assetUrl(triple);
  try {
    await download(url, dest);
    if (process.platform !== "win32") {
      fs.chmodSync(dest, 0o755);
    }
    console.log(`[dross] installed ${binaryName()} for ${triple}`);
  } catch (error) {
    console.warn(
      `[dross] could not download the binary (${error.message}).\n` +
        `        Fetch it manually from https://github.com/${REPO}/releases/tag/v${pkg.version}\n` +
        `        and place it at ${dest}, or build from source with:\n` +
        "        cargo build --release -p dross-cli",
    );
  }
}

main();
