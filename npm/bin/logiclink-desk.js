#!/usr/bin/env node

const { spawnSync } = require("node:child_process");
const { existsSync } = require("node:fs");
const { join } = require("node:path");

const binaryName = process.platform === "win32" ? "logiclink-desk.exe" : "logiclink-desk";
const binaryPath = join(
  __dirname,
  "..",
  "..",
  "prebuilt",
  `${process.platform}-${process.arch}`,
  binaryName,
);

if (!existsSync(binaryPath)) {
  console.error(
    [
      `logiclink-desk does not include a prebuilt binary for ${process.platform}-${process.arch}.`,
      "Install a package version that supports this platform or build the Rust binary from source.",
    ].join("\n"),
  );
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

if (result.signal) {
  process.kill(process.pid, result.signal);
}

process.exit(result.status ?? 1);
