// Compare output size and SSIMULACRA2 of both pipelines against a neutral
// reference (make-reference.py: Pillow, full-resolution Lanczos, upright, sRGB).
//   bun quality.ts [--max 320] [--format webp]
import { mkdirSync, readdirSync, statSync, existsSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import sharp from 'sharp';
import { JXL_AVAILABLE } from './baseline.ts';

const args = new Map<string, string>();
for (let i = 2; i < process.argv.length; i += 2) args.set(process.argv[i].replace(/^--/, ''), process.argv[i + 1]);
const requested = (args.get('format') ?? 'webp,avif,jxl').split(',');
if (!JXL_AVAILABLE && requested.includes('jxl')) console.error('BASELINE_JXL_MODULE is not set: skipping JPEG XL (no baseline to compare against)');
const formats = requested.filter((f) => f !== 'jxl' || JXL_AVAILABLE);
const maxes = (args.get('max') ?? '320,1920').split(',').map(Number);
const corpus = join(import.meta.dir, 'images/corpus');
const work = join(import.meta.dir, 'out/quality');
const ssim2 = join(import.meta.dir, '../target/release/examples/ssim2');
const names = readdirSync(corpus).filter((f) => !f.endsWith('.json') && !f.startsWith('anim-')).sort();

async function run(cmd: string[]) {
  const p = Bun.spawn(cmd, { stdout: 'pipe', stderr: 'inherit' });
  const out = await new Response(p.stdout).text();
  if ((await p.exited) !== 0) throw new Error(`${cmd.join(' ')} failed`);
  return out;
}

const summary: Record<string, unknown>[] = [];
for (const max of maxes) {
  const refDir = join(work, `ref-${max}`);
  await run(['python3', join(import.meta.dir, 'make-reference.py'), String(max), refDir]);
  for (const format of formats) {
    const scores: Record<string, Record<string, number | string>> = {};
    const bytes: Record<string, number> = {};
    for (const impl of ['sharp', 'imgconv']) {
      const dir = join(work, `${impl}-${format}-${max}`);
      await run(['bun', join(import.meta.dir, 'run-one.ts'), '--impl', impl, '--format', format, '--max', String(max),
        '--concurrency', '4', '--passes', '1', '--keep', dir, '--only', '^(?!anim-)']);
      const outDir = join(dir, 'pass0');
      bytes[impl] = 0;
      for (const n of names) {
        const base = n.replace(/\.[^.]+$/, '');
        const file = readdirSync(outDir).find((f) => f.startsWith(`${base}.`));
        if (!file) { (scores[n] ??= {})[impl] = 'missing'; continue; }
        bytes[impl] += statSync(join(outDir, file)).size;
        let cand = join(outDir, file);
        if (!file.endsWith('.jxl')) {
          const png = `${cand}.png`;
          await sharp(cand).png().toFile(png);
          cand = png;
        }
        const line = (await run([ssim2, join(refDir, `${n}.png`), cand])).trim();
        const [score, , ...rest] = line.split(' ');
        (scores[n] ??= {})[impl] = score === 'NaN' ? `NaN (${rest.join(' ')})` : Number(score);
      }
    }
    const mean = (impl: string) => {
      const v = Object.values(scores).map((s) => s[impl]).filter((x): x is number => typeof x === 'number');
      return Math.round((v.reduce((a, b) => a + b, 0) / v.length) * 100) / 100;
    };
    const row = { format, max, bytes, mean_ssimulacra2: { sharp: mean('sharp'), imgconv: mean('imgconv') }, per_file: scores };
    summary.push(row);
    console.log(JSON.stringify({ format, max, bytes, mean: row.mean_ssimulacra2 }));
  }
}
mkdirSync(join(import.meta.dir, 'out'), { recursive: true });
writeFileSync(join(import.meta.dir, 'out/quality.json'), JSON.stringify(summary, null, 2));
