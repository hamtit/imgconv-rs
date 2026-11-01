// One benchmark scenario in a fresh process; prints a JSON line.
//   bun run-one.ts --impl sharp|imgconv --format webp|avif|jxl --max 320 --concurrency 10 [--threads 1] [--passes 2] [--keep DIR]
// `sharp` runs the sharp baseline (baseline.ts); `imgconv` runs this library.
import { mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';

const args = new Map<string, string>();
for (let i = 2; i < process.argv.length; i += 2) args.set(process.argv[i].replace(/^--/, ''), process.argv[i + 1]);
const impl = args.get('impl') ?? 'imgconv';
const format = (args.get('format') ?? 'webp') as 'webp' | 'avif' | 'jxl';
const max = Number(args.get('max') ?? 0) || undefined;
const concurrency = Number(args.get('concurrency') ?? 10);
const threads = Number(args.get('threads') ?? 1);
const passes = Number(args.get('passes') ?? 2);
const only = args.get('only');

const keep = args.get('keep');
const staticDir = keep ?? mkdtempSync(join(tmpdir(), 'imgconv-bench-'));

function status(key: string): number {
  const m = readFileSync('/proc/self/status', 'utf8').match(new RegExp(`^${key}:\\s+(\\d+) kB`, 'm'));
  return m ? Number(m[1]) * 1024 : 0;
}
const mb = (b: number) => Math.round((b / 1048576) * 10) / 10;

const corpusDir = join(import.meta.dir, 'images/corpus');
let names = readdirSync(corpusDir).filter((f) => !f.endsWith('.json')).sort();
if (only) names = names.filter((n) => new RegExp(only).test(n));
const inputs = names.map((n) => ({ name: n, buf: readFileSync(join(corpusDir, n)) }));

const rssBeforeImport = status('VmRSS');
async function saveFile(path: string, data: Uint8Array) {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, data);
}
let convertOne: (name: string, buf: Buffer, outBase: string) => Promise<void>;
if (impl === 'sharp') {
  const enc = await import('./baseline.ts');
  convertOne = async (_name, buf, outBase) => {
    if (buf[0] === 0x47 && buf[1] === 0x49 && buf[2] === 0x46) {
      await enc.validateInputPixels(buf);
      enc.validateGifCanvasPixels(buf);
      if ((await enc.countGifFrames(buf)) > 1) {
        await enc.encodeAnimatedGif(buf, `${outBase}.webp`, max);
        return;
      }
    }
    if (format === 'jxl') await enc.encodeJxl(buf, `${outBase}.jxl`, max);
    else await enc.encodeWithSharp(buf, `${outBase}.${format}`, format, max);
  };
} else {
  const imgconv = require(join(import.meta.dir, '..'));
  convertOne = async (_name, buf, outBase) => {
    const r = await imgconv.convert(buf, { format, quality: 65, maxDimension: max, threads });
    await saveFile(`${outBase}.${r.format}`, r.data);
  };
}
Bun.gc(true);
const rssAfterImport = status('VmRSS');

async function runPass(pass: number) {
  const dir = join(staticDir, `pass${pass}`);
  let next = 0;
  const latencies: Record<string, number> = {};
  const failures: string[] = [];
  const worker = async () => {
    while (next < inputs.length) {
      const { name, buf } = inputs[next++];
      const t = performance.now();
      try {
        await convertOne(name, buf, join(dir, name.replace(/\.[^.]+$/, '')));
      } catch (e) {
        failures.push(`${name}: ${e instanceof Error ? e.message : e}`);
      }
      latencies[name] = performance.now() - t;
    }
  };
  const cpu0 = process.cpuUsage();
  const t0 = performance.now();
  await Promise.all(Array.from({ length: Math.min(concurrency, inputs.length) }, worker));
  const wall = performance.now() - t0;
  const cpu = process.cpuUsage(cpu0);
  let bytes = 0;
  for (const f of readdirSync(dir)) bytes += statSync(join(dir, f)).size;
  return { wall_ms: Math.round(wall), cpu_ms: Math.round((cpu.user + cpu.system) / 1000), out_bytes: bytes, latencies, failures };
}

const results = [];
for (let p = 0; p < passes; p++) results.push(await runPass(p));
const peak = status('VmHWM');
await Bun.sleep(1000);
Bun.gc(true);
const rssSettled = status('VmRSS');

console.log(JSON.stringify({
  impl, format, max: max ?? null, concurrency, threads, files: inputs.length,
  rss_before_import_mb: mb(rssBeforeImport),
  rss_after_import_mb: mb(rssAfterImport),
  peak_rss_mb: mb(peak),
  rss_settled_mb: mb(rssSettled),
  passes: results,
}));
if (!keep) rmSync(staticDir, { recursive: true, force: true });
process.exit(0);
