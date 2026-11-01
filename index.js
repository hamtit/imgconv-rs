'use strict';
// Loads the prebuilt Node-API addon for this platform (Node.js and Bun).
const { join } = require('node:path');
const { existsSync } = require('node:fs');

function platformTriple() {
  const arch = process.arch;
  if (process.platform !== 'linux' || (arch !== 'x64' && arch !== 'arm64')) {
    throw new Error(`imgconv-rs: unsupported platform ${process.platform}-${arch}`);
  }
  const report = typeof process.report?.getReport === 'function' ? process.report.getReport() : null;
  const glibc = report?.header?.glibcVersionRuntime;
  // Bun does not implement process.report; treat a missing report as glibc
  // unless the musl loader is present.
  const musl = glibc ? false : existsSync(`/lib/ld-musl-${arch === 'x64' ? 'x86_64' : 'aarch64'}.so.1`);
  return `linux-${arch}-${musl ? 'musl' : 'gnu'}`;
}

const triple = platformTriple();
const file = join(__dirname, `imgconv-rs.${triple}.node`);
if (!existsSync(file)) {
  throw new Error(`imgconv-rs: no prebuilt binary for ${triple} (${file})`);
}
module.exports = require(file);
