#!/usr/bin/env node
// Assemble an npm package from build outputs and `npm pack` it.
//
//   node scripts/pack.mjs native      <out-dir> <addon-x64.node> <addon-arm64.node>
//   node scripts/pack.mjs native-arch <out-dir> <x64|arm64> <addon.node>
//   node scripts/pack.mjs wasm        <out-dir> <wasm-build-dir>
//
// <addon.node> is imgconv-rs.linux-<arch>-gnu.node; <wasm-build-dir> is the
// output of scripts/build-wasm.sh. licenses/ (scripts/collect-licenses.sh)
// must exist for the native kinds. The version is the one in Cargo.toml.
//
// Writes <out-dir>/imgconv-rs[-linux-<arch>|-wasm]-<version>.tgz
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export function cargoVersion() {
  const toml = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
  const m = toml.match(/^\[package\][^[]*?^version\s*=\s*"([^"]+)"/ms);
  if (!m) throw new Error('version not found in Cargo.toml');
  return m[1];
}

function copy(from, to) {
  fs.cpSync(from, to, { recursive: true, dereference: true });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [kind, outDir, ...inputs] = process.argv.slice(2);
  const version = cargoVersion();
  const stage = fs.mkdtempSync(path.join(os.tmpdir(), `imgconv-${kind}-`));
  let pkgName = 'imgconv-rs';
  let pkg;
  const base = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));

  // Fields shared by the three native packages; scripts and devDependencies
  // belong to the repository only.
  const nativePackage = (name, cpu) => ({
    name,
    version,
    description: base.description,
    license: base.license,
    repository: { type: 'git', url: 'git+https://github.com/hamtit/imgconv-rs.git' },
    main: base.main,
    types: base.types,
    files: base.files,
    os: base.os,
    cpu,
    private: true,
  });

  if (kind === 'native' || kind === 'native-arch') {
    let addons; // [arch, file]
    if (kind === 'native') {
      addons = [['x64', inputs[0]], ['arm64', inputs[1]]];
      pkg = nativePackage('imgconv-rs', base.cpu);
    } else {
      const arch = inputs[0];
      if (!['x64', 'arm64'].includes(arch)) throw new Error(`unknown arch: ${arch}`);
      addons = [[arch, inputs[1]]];
      pkgName = `imgconv-rs-linux-${arch}`;
      pkg = nativePackage(pkgName, [arch]);
    }
    for (const f of ['index.js', 'index.d.ts', 'LICENSE']) copy(path.join(root, f), path.join(stage, f));
    copy(path.join(root, 'licenses'), path.join(stage, 'licenses'));
    for (const [arch, file] of addons) {
      copy(path.resolve(file), path.join(stage, `imgconv-rs.linux-${arch}-gnu.node`));
    }
    fs.writeFileSync(
      path.join(stage, 'README.md'),
      `# ${pkg.name}\n\n` +
        (kind === 'native'
          ? 'Node-API addon for Linux x64 and arm64 (glibc).\n'
          : `Node-API addon for Linux ${inputs[0]} only (glibc); the \`os\`/\`cpu\` fields let package managers skip it elsewhere.\n`) +
        '\nSee https://github.com/hamtit/imgconv-rs for usage.\n',
    );
  } else if (kind === 'wasm') {
    const dir = path.resolve(inputs[0]);
    pkgName = 'imgconv-rs-wasm';
    const template = path.join(root, 'packages', 'wasm');
    for (const f of ['index.js', 'index.d.ts', 'README.md']) copy(path.join(template, f), path.join(stage, f));
    copy(path.join(dir, 'dist'), path.join(stage, 'dist'));
    copy(path.join(dir, 'licenses'), path.join(stage, 'licenses'));
    copy(path.join(root, 'LICENSE'), path.join(stage, 'LICENSE'));
    pkg = JSON.parse(fs.readFileSync(path.join(template, 'package.json'), 'utf8'));
    pkg.version = version;
  } else {
    throw new Error(`unknown package kind: ${kind}`);
  }
  fs.writeFileSync(path.join(stage, 'package.json'), JSON.stringify(pkg, null, 2) + '\n');

  fs.mkdirSync(outDir, { recursive: true });
  const dest = path.resolve(outDir);
  const [{ filename }] = JSON.parse(
    execFileSync('npm', ['pack', '--json', '--pack-destination', dest], { cwd: stage, encoding: 'utf8' }),
  );
  const expected = `${pkgName}-${version}.tgz`;
  if (path.basename(filename) !== expected) {
    fs.renameSync(path.join(dest, path.basename(filename)), path.join(dest, expected));
  }
  fs.rmSync(stage, { recursive: true, force: true });
  console.log(path.join(dest, expected));
}
