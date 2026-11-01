// Run the benchmark matrix (each run in a fresh process) and write
// out/results.json and out/results.md.
//   bun run-all.ts [--reps 3]
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { cpus } from 'node:os';
import { JXL_AVAILABLE } from './baseline.ts';

const args = new Map<string, string>();
for (let i = 2; i < process.argv.length; i += 2) args.set(process.argv[i].replace(/^--/, ''), process.argv[i + 1]);
const reps = Number(args.get('reps') ?? 3);

async function runOne(opts: Record<string, string | number>) {
  const cmd = ['bun', join(import.meta.dir, 'run-one.ts'), ...Object.entries(opts).flatMap(([k, v]) => [`--${k}`, String(v)])];
  const p = Bun.spawn(cmd, { stdout: 'pipe', stderr: 'inherit' });
  const out = await new Response(p.stdout).text();
  if ((await p.exited) !== 0) throw new Error(`${cmd.join(' ')} failed`);
  return JSON.parse(out.trim().split('\n').at(-1)!);
}

if (!JXL_AVAILABLE) console.error('BASELINE_JXL_MODULE is not set: skipping JPEG XL scenarios (no baseline to compare against)');
const FORMATS = JXL_AVAILABLE ? ['webp', 'avif', 'jxl'] : ['webp', 'avif'];

const median = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];

type Row = Record<string, unknown>;
const rows: Row[] = [];
async function scenario(label: string, opts: Record<string, string | number>, n = reps) {
  const runs = [];
  for (let i = 0; i < n; i++) runs.push(await runOne(opts));
  const failures = runs.flatMap((r) => r.passes.flatMap((p: { failures: string[] }) => p.failures));
  const row = {
    label, ...opts, reps: n,
    cold_wall_ms: median(runs.map((r) => r.passes[0].wall_ms)),
    warm_wall_ms: runs[0].passes[1] ? median(runs.map((r) => r.passes[1].wall_ms)) : null,
    cold_cpu_ms: median(runs.map((r) => r.passes[0].cpu_ms)),
    peak_rss_mb: median(runs.map((r) => r.peak_rss_mb)),
    rss_after_import_mb: median(runs.map((r) => r.rss_after_import_mb)),
    rss_settled_mb: median(runs.map((r) => r.rss_settled_mb)),
    out_bytes: runs[0].passes[0].out_bytes,
    files: runs[0].files,
    failures,
  };
  rows.push(row);
  console.error(JSON.stringify(row));
}

for (const format of FORMATS) {
  for (const max of [320, 1920]) {
    for (const impl of ['sharp', 'imgconv']) {
      await scenario('burst', { impl, format, max, concurrency: 10, passes: 2 });
    }
  }
}
// One image at a time: latency, and whether intra-image threads help.
for (const format of FORMATS.filter((f) => f !== 'webp')) {
  await scenario('serial', { impl: 'sharp', format, max: 1920, concurrency: 1, passes: 1 }, 1);
  await scenario('serial', { impl: 'imgconv', format, max: 1920, concurrency: 1, passes: 1, threads: 1 }, 1);
  await scenario('serial', { impl: 'imgconv', format, max: 1920, concurrency: 1, passes: 1, threads: cpus().length }, 1);
}

mkdirSync(join(import.meta.dir, 'out'), { recursive: true });
const meta = { date: new Date().toISOString(), cpus: cpus().length, cpu: cpus()[0]?.model, bun: Bun.version };
writeFileSync(join(import.meta.dir, 'out/results.json'), JSON.stringify({ meta, rows }, null, 2));

const pct = (a: number, b: number) => `${b < a ? '-' : '+'}${Math.abs(Math.round((1 - b / a) * 100))}%`;
let md = `| scenario | format | max | sharp wall (cold/warm) | imgconv wall (cold/warm) | sharp CPU | imgconv CPU | sharp peak RSS | imgconv peak RSS | sharp RSS after | imgconv RSS after | output bytes sharp / imgconv |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n`;
for (const format of FORMATS) {
  for (const max of [320, 1920]) {
    const s = rows.find((r) => r.label === 'burst' && r.impl === 'sharp' && r.format === format && r.max === max) as any;
    const h = rows.find((r) => r.label === 'burst' && r.impl === 'imgconv' && r.format === format && r.max === max) as any;
    md += `| burst x10 | ${format} | ${max} | ${s.cold_wall_ms} / ${s.warm_wall_ms} ms | ${h.cold_wall_ms} / ${h.warm_wall_ms} ms (${pct(s.cold_wall_ms, h.cold_wall_ms)}) | ${s.cold_cpu_ms} ms | ${h.cold_cpu_ms} ms (${pct(s.cold_cpu_ms, h.cold_cpu_ms)}) | ${s.peak_rss_mb} MB | ${h.peak_rss_mb} MB (${pct(s.peak_rss_mb, h.peak_rss_mb)}) | ${s.rss_settled_mb} MB | ${h.rss_settled_mb} MB | ${s.out_bytes} / ${h.out_bytes} |\n`;
  }
}
md += `\n| serial | impl | threads | wall | CPU | peak RSS |\n|---|---|---|---|---|---|\n`;
for (const r of rows.filter((r) => r.label === 'serial') as any[]) {
  md += `| ${r.format} ${r.max} | ${r.impl} | ${r.threads ?? '-'} | ${r.cold_wall_ms} ms | ${r.cold_cpu_ms} ms | ${r.peak_rss_mb} MB |\n`;
}
writeFileSync(join(import.meta.dir, 'out/results.md'), md);
console.log(md);
