#!/usr/bin/env node
// Syncs one version across tauri.conf.json / Cargo.toml / package.json.
// Usage: node scripts/bump.mjs 0.2.0

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(-[\w.-]+)?$/.test(version)) {
  console.error("Usage: node scripts/bump.mjs <semver>");
  process.exit(1);
}

const root = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const read = (p) => readFileSync(join(root, p), "utf8");

// tauri.conf.json — the single source of truth.
const tauriPath = "src-tauri/tauri.conf.json";
const tauri = JSON.parse(read(tauriPath));
tauri.version = version;
writeFileSync(join(root, tauriPath), JSON.stringify(tauri, null, 2) + "\n");

// package.json (skip if name is private placeholder only)
try {
  const pkg = JSON.parse(read("package.json"));
  pkg.version = version;
  writeFileSync(join(root, "package.json"), JSON.stringify(pkg, null, 2) + "\n");
} catch { /* optional */ }

// Cargo.toml — replace the [package] version line.
const cargoPath = "src-tauri/Cargo.toml";
const cargo = read(cargoPath).replace(
  /^version\s*=\s*"[^"]*"/m,
  `version = "${version}"`
);
writeFileSync(join(root, cargoPath), cargo);

console.log(`Bumped to ${version}`);
