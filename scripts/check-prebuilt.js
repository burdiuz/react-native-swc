#!/usr/bin/env node
// Guard for `prepublishOnly`: the native libraries are git-ignored build output that must be in the
// npm tarball. Fails if none were built, or if one is older than the Rust sources it was built from.
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const jniLibs = path.join(root, 'android/src/main/jniLibs');
const newest = (dir) =>
  fs.readdirSync(dir, { withFileTypes: true }).reduce((max, e) => {
    if (e.name === 'target') return max;
    const p = path.join(dir, e.name);
    return Math.max(max, e.isDirectory() ? newest(p) : fs.statSync(p).mtimeMs);
  }, 0);

const abis = fs.existsSync(jniLibs) ? fs.readdirSync(jniLibs).filter((a) => fs.existsSync(path.join(jniLibs, a, 'libactualwave_swc.so'))) : [];
if (abis.length === 0) {
  console.error('No libactualwave_swc.so under android/src/main/jniLibs — run `npm run build:android` first.');
  process.exit(1);
}
const srcTime = newest(path.join(root, 'rust'));
const stale = abis.filter((a) => fs.statSync(path.join(jniLibs, a, 'libactualwave_swc.so')).mtimeMs < srcTime);
if (stale.length) {
  console.error(`Stale native library for: ${stale.join(', ')} — rust/ changed since it was built. Re-run \`npm run build:android\`.`);
  process.exit(1);
}
console.log(`Prebuilt libraries OK: ${abis.join(', ')}`);
