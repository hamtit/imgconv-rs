// Compare builds of this library (before/after an optimisation).
//   bun compare.ts label=/path/a.node label=/path/b.node [--reps 3] [--out file.md]
// Each scenario runs in a fresh process (run-one.ts, IMGCONV_NODE selects the
// addon); medians over --reps are printed as a markdown table.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

const builds: [string, string][] = [];
let reps = 3;
let outFile: string | undefined;
let only: RegExp | undefined;
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === '--reps') reps = Number(argv[++i]);
  else if (argv[i] === '--out') outFile = argv[++i];
  else if (argv[i] === '--only') only = new RegExp(argv[++i]);
  else if (argv[i].includes('=')) builds.push(argv[i].split('=', 2) as [string, string]);
}


type Scenario = { name: string; opts: Record<string, string | number> };
const scenarios: Scenario[] = [];
for (const format of ['webp', 'avif', 'jxl']) {
  for (const max of [320, 1920]) {
    scenarios.push({ name: `corpus x10 ${format} ${max}`, opts: { format, max, concurrency: 10, passes: 2 } });
  }
}
for (const format of ['webp', 'avif']) {
  scenarios.push({ name: `large PNG/JPEG x4 ${format} 1920`, opts: { format, max: 1920, concurrency: 4, passes: 1, corpus: 'large' } });
  scenarios.push({ name: `large PNG/JPEG x1 ${format} 1920`, opts: { format, max: 1920, concurrency: 1, passes: 1, corpus: 'large' } });
}
scenarios.push({ name: 'large x4 webp no-resize', opts: { format: 'webp', max: 0, concurrency: 4, passes: 1, corpus: 'large' } });

const median = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
async function runOne(node: string, opts: Record<string, string | number>) {
  const cmd = ['bun', join(import.meta.dir, 'run-one.ts'), '--impl', 'imgconv', ...Object.entries(opts).flatMap(([k, v]) => [`--${k}`, String(v)])];
  const p = Bun.spawn(cmd, { stdout: 'pipe', stderr: 'inherit', env: { ...process.env, IMGCONV_NODE: node } });
  const out = await new Response(p.stdout).text();
  if ((await p.exited) !== 0) throw new Error(`${cmd.join(' ')} failed`);
  return JSON.parse(out.trim().split('\n').at(-1)!);
}

let md = `| scenario | build | wall ms | CPU ms | peak RSS MB | peak − idle MB | settled RSS MB | out bytes | fail |\n|---|---|---|---|---|---|---|---|---|\n`;
for (const sc of scenarios) {
  if (only && !only.test(sc.name)) continue;
  for (const [label, node] of builds) {
    const runs = [];
    for (let i = 0; i < reps; i++) runs.push(await runOne(node, sc.opts));
    const last = (r: any) => r.passes[r.passes.length - 1];
    const row = [
      sc.name, label,
      median(runs.map((r) => r.passes[0].wall_ms)),
      median(runs.map((r) => r.passes[0].cpu_ms)),
      median(runs.map((r) => r.peak_rss_mb)),
      Math.round(median(runs.map((r) => r.peak_rss_mb - r.rss_after_import_mb)) * 10) / 10,
      median(runs.map((r) => r.rss_settled_mb)),
      runs[0].passes[0].out_bytes,
      runs.reduce((n, r) => n + last(r).failures.length, 0),
    ];
    const line = `| ${row.join(' | ')} |`;
    md += line + '\n';
    console.error(line);
  }
}
console.log(md);
if (outFile) writeFileSync(outFile, md);
