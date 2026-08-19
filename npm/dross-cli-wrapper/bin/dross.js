#!/usr/bin/env node
"use strict";

// Launcher: forwards every argument and the exit code to the native binary,
// so `npx dross check --staged --hook` still blocks a commit correctly.

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");

const { binaryPath } = require("../scripts/platform");

const bin = binaryPath();

if (!fs.existsSync(bin)) {
  console.error(
    "[dross] the native binary is missing.\n" +
      "        Reinstall the package, download it from\n" +
      "        https://github.com/Sarthak-47/Dross/releases,\n" +
      "        or build from source: cargo build --release -p dross-cli",
  );
  process.exit(127);
}

const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });

if (result.error) {
  console.error(`[dross] failed to run: ${result.error.message}`);
  process.exit(126);
}

// A signal-terminated child has a null status; report it as a shell would.
process.exit(result.status === null ? 128 : result.status);
