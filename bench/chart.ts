// bench/results.csv (from sweep.sh) -> one SVG per metric, x = concurrent users, one line per server config.
// Run with Node >= 22.18 (it executes TypeScript directly): node bench/chart.ts
import { readFileSync, writeFileSync } from 'node:fs';

const dir = new URL('.', import.meta.url).pathname;
const NAMES: Record<string, string> = {
  rs: 'pdfrs, all cores', jsN: 'pdfjs Node × cores', js1: 'pdfjs Node × 1', rs1: 'pdfrs, 1 core', js1c: 'pdfjs Node × 1, 1 core',
};
type Row = { label: string; vus: number; reqS: number; p95: number; mem: number };
const rows: Row[] = readFileSync(dir + 'results.csv', 'utf8').trim().split('\n').slice(1).map((l) => l.split(','))
  .map(([s, v, r, p, , m]) => ({ label: NAMES[s] ?? s, vus: +v, reqS: +r, p95: +p, mem: +m }));
const vus = [...new Set(rows.map((r) => r.vus))];
const labels = [...new Set(rows.map((r) => r.label))];
const COLORS = [['#2a78d6', '#3987e5'], ['#eb6834', '#d95926'], ['#1baf7a', '#199e70'], ['#eda100', '#c98500'], ['#8a5cd6', '#9b6fe3']];

function chart(file: string, title: string, key: 'reqS' | 'mem' | 'p95', unit: string, log: boolean) {
  const series = labels.map((label, i) => ({ label, cls: `s${i + 1}`, pts: rows.filter((r) => r.label === label).map((r) => ({ v: r.vus, val: r[key] })) }));
  const W = 800, H = 400, L = 60, R = 250, T = 76, B = 48;
  const vals = rows.map((r) => r[key]).filter((v) => v > 0);
  let ticks: number[], y: (v: number) => number;
  if (log) {
    const lo = 10 ** Math.floor(Math.log10(Math.min(...vals))), hi = 10 ** Math.ceil(Math.log10(Math.max(...vals)));
    ticks = [];
    for (let d = lo; d <= hi; d *= 10) ticks.push(...[d, d * 3].filter((t) => t <= hi));
    y = (v) => T + (H - T - B) * (1 - (Math.log10(Math.max(v, lo)) - Math.log10(lo)) / (Math.log10(hi) - Math.log10(lo)));
  } else {
    const top = Math.max(...vals);
    const step = [50, 100, 250, 500, 1000, 2500].find((s) => top / s <= 5) ?? 5000;
    const max = Math.ceil(top / step) * step;
    ticks = Array.from({ length: max / step + 1 }, (_, i) => i * step);
    y = (v) => T + (H - T - B) * (1 - v / max);
  }
  const x = (v: number) => L + (vus.indexOf(v) * (W - L - R)) / (vus.length - 1);
  // end-of-line labels: sorted by height and pushed apart so they never overlap
  const ends = series.map((s) => ({ s, ly: y(s.pts.at(-1)!.val) + 4 })).sort((a, b) => a.ly - b.ly);
  ends.forEach((e, i) => { if (i) e.ly = Math.max(e.ly, ends[i - 1].ly + 15); });
  const fmt = (n: number) => n.toLocaleString('en-US');

  writeFileSync(dir + file, `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" font-family="-apple-system, Segoe UI, Helvetica, sans-serif" font-size="12">
<style>
  .bg{fill:#fcfcfb} .ink{fill:#0b0b0b} .muted{fill:#898781} .grid{stroke:#e1e0d9} .axis{stroke:#c3c2b7}
  ${COLORS.map(([c], i) => `.s${i + 1}{stroke:${c};fill:${c}}`).join(' ')}
  @media (prefers-color-scheme: dark){
    .bg{fill:#1a1a19} .ink{fill:#f0efec} .grid{stroke:#2c2c2a} .axis{stroke:#383835}
    ${COLORS.map(([, c], i) => `.s${i + 1}{stroke:${c};fill:${c}}`).join(' ')}
  }
</style>
<rect class="bg" width="${W}" height="${H}" rx="8"/>
<text class="ink" x="${L}" y="22" font-size="14" font-weight="600">${title}</text>
${series.map((s, i) => `<g transform="translate(${L + (i % 3) * 190},${42 + Math.floor(i / 3) * 18})"><line class="${s.cls}" x1="0" x2="14" y1="0" y2="0" stroke-width="2"/><circle class="${s.cls}" cx="7" r="4"/><text class="muted" x="20" y="4">${s.label}</text></g>`).join('')}
${ticks.map((t) => `<line class="${t === ticks[0] ? 'axis' : 'grid'}" x1="${L}" x2="${W - R}" y1="${y(t)}" y2="${y(t)}"/><text class="muted" x="${L - 8}" y="${y(t) + 4}" text-anchor="end">${fmt(t)}</text>`).join('\n')}
${vus.map((v) => `<text class="muted" x="${x(v)}" y="${H - B + 18}" text-anchor="middle">${v}</text>`).join('')}
<text class="muted" x="${(L + W - R) / 2}" y="${H - 8}" text-anchor="middle">k6 virtual users (VUs)${log ? ' — log scale' : ''}</text>
${series.map((s) => `<polyline class="${s.cls}" style="fill:none" stroke-width="2" stroke-linejoin="round" points="${s.pts.map((p) => `${x(p.v)},${y(p.val)}`).join(' ')}"/>
${s.pts.map((p) => `<circle class="${s.cls}" cx="${x(p.v)}" cy="${y(p.val)}" r="4"><title>${s.label}, ${p.v} VUs: ${fmt(p.val)} ${unit}</title></circle>`).join('')}`).join('\n')}
${ends.map(({ s, ly }) => `<text class="ink" x="${W - R + 10}" y="${ly}">${s.label} · ${fmt(s.pts.at(-1)!.val)} ${unit}</text>`).join('\n')}
</svg>
`);
}

chart('chart.svg', 'Throughput (PDF/s) by number of concurrent users', 'reqS', 'PDF/s', true);
chart('memory.svg', 'Peak memory (MB) by number of concurrent users', 'mem', 'MB', false);
chart('latency.svg', 'p95 latency (ms) by number of concurrent users', 'p95', 'ms', true);
