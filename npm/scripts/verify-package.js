#!/usr/bin/env node

const { existsSync, readdirSync, statSync } = require("node:fs");
const { join } = require("node:path");

const prebuiltDirectory = "prebuilt";

if (!existsSync(prebuiltDirectory)) {
  console.error("Missing prebuilt binaries. Run npm run prepare:binary before npm pack or npm publish.");
  process.exit(1);
}

const packagedBinaries = readdirSync(prebuiltDirectory)
  .flatMap((platformDirectory) => {
    const directory = join(prebuiltDirectory, platformDirectory);

    if (!statSync(directory).isDirectory()) {
      return [];
    }

    return readdirSync(directory)
      .filter((fileName) => fileName === "logiclink-desk" || fileName === "logiclink-desk.exe")
      .map((fileName) => join(directory, fileName));
  });

if (packagedBinaries.length === 0) {
  console.error("No prebuilt logiclink-desk binaries found. Run npm run prepare:binary before publishing.");
  process.exit(1);
}

console.log(`Found ${packagedBinaries.length} prebuilt binary package entry.`);
