"use strict";

const os = require("node:os");
const path = require("node:path");

/** Maps the running platform to a release asset target triple. */
function targetTriple() {
  const platform = os.platform();
  const arch = os.arch();

  if (platform === "win32" && arch === "x64") {
    return "x86_64-pc-windows-msvc";
  }
  if (platform === "darwin" && arch === "arm64") {
    return "aarch64-apple-darwin";
  }
  if (platform === "darwin" && arch === "x64") {
    return "x86_64-apple-darwin";
  }
  if (platform === "linux" && arch === "x64") {
    return "x86_64-unknown-linux-gnu";
  }
  return null;
}

function binaryName() {
  return os.platform() === "win32" ? "dross.exe" : "dross";
}

function binaryPath() {
  return path.join(__dirname, "..", "bin", binaryName());
}

module.exports = { targetTriple, binaryName, binaryPath };
