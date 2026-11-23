// Keeps the app version in lockstep across every version manifest.
// Usage: node scripts/version.mjs <version>   |   node scripts/version.mjs --check
import { readFileSync, writeFileSync } from "node:fs";

const files = {
  "package.json": [/("version":\s*")([^"]+)(")/],
  "src-tauri/tauri.conf.json": [/("version":\s*")([^"]+)(")/],
  "src-tauri/Cargo.toml": [/(^version\s*=\s*")([^"]+)(")/m],
  "gloria.toml": [/(^version\s*=\s*")([^"]+)(")/m],
};

const read = (f) => readFileSync(f, "utf8");
const versionOf = (f) => read(f).match(files[f][0])?.[2];
const arg = process.argv[2];

if (!arg || arg === "--check") {
  const seen = Object.keys(files).map((f) => [f, versionOf(f)]);
  const unique = new Set(seen.map(([, v]) => v));
  for (const [f, v] of seen) console.log(`${v}\t${f}`);
  if (unique.size !== 1) {
    console.error("Version mismatch across manifests.");
    process.exit(1);
  }
} else {
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/.test(arg)) {
    console.error(`Invalid version: ${arg}`);
    process.exit(1);
  }
  for (const f of Object.keys(files)) {
    writeFileSync(f, read(f).replace(files[f][0], `$1${arg}$3`));
  }
  console.log(`Set version ${arg}`);
}
