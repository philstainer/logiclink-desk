#!/usr/bin/env node

const { chmodSync, copyFileSync, existsSync, mkdirSync } = require("node:fs");
const { join } = require("node:path");

const binaryName = process.platform === "win32" ? "logiclink-desk.exe" : "logiclink-desk";
const source = join("target", "release", binaryName);
const destinationDirectory = join("prebuilt", `${process.platform}-${process.arch}`);
const destination = join(destinationDirectory, binaryName);

if (!existsSync(source)) {
  console.error(`Missing ${source}. Run npm run build:release first.`);
  process.exit(1);
}

mkdirSync(destinationDirectory, { recursive: true });
copyFileSync(source, destination);

if (process.platform !== "win32") {
  chmodSync(destination, 0o755);
}

console.log(`Packaged ${destination}`);
