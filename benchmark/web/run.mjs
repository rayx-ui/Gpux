// Runs benchmark pages in Playwright's Chromium and prints one JSON report per run.
//
//   node web/run.mjs --plan <plan.json>
//
// `plan.json` is { dist: <dir with index.html and the wasm-bindgen output>, runs: [{ query, out }] }.
// Each run opens a fresh page of the served dist at `/?<query>` in one headed browser (a visible window, so frames are paced by the display
// and drawn by the GPU), waits for the benchmark to report, adds Chrome's own main-thread
// timings over the DevTools protocol for the measured frames (the page cannot read its CPU
// time), and writes the merged report to `out`.
//
// Playwright comes from the repository's tools-node install (`pnpm install` there, then
// `pnpm exec playwright install chromium`).

import { createRequire } from 'node:module';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(path.join(here, '..', '..', 'tools-node', 'package.json'));
const { chromium } = require('@playwright/test');

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, index, all) =>
    arg.startsWith('--') ? [...pairs, [arg.slice(2), all[index + 1]]] : pairs, []),
);
const plan = JSON.parse(fs.readFileSync(args.plan, 'utf8'));
const dist = path.resolve(plan.dist);
const runs = plan.runs;
const timeoutMs = Number(args.timeout ?? 180000);

const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.json': 'application/json' };
const server = http.createServer((request, response) => {
  const url = new URL(request.url, 'http://localhost');
  let file = path.join(dist, decodeURIComponent(url.pathname));
  if (!file.startsWith(dist) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
    file = path.join(dist, 'index.html');
  }
  response.writeHead(200, {
    'Content-Type': types[path.extname(file)] ?? 'application/octet-stream',
    'Cross-Origin-Opener-Policy': 'same-origin',
    'Cross-Origin-Embedder-Policy': 'require-corp',
    'Cross-Origin-Resource-Policy': 'same-origin',
    'Cache-Control': 'no-store',
  });
  fs.createReadStream(file).pipe(response);
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;

const browser = await chromium.launch({
  headless: false,
  args: ['--enable-unsafe-webgpu', '--enable-features=Vulkan', '--disable-background-timer-throttling', '--disable-renderer-backgrounding'],
});
console.log(`browser ${browser.version()}`);
const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 });

const metric = (metrics, name) => metrics.find((entry) => entry.name === name)?.value ?? 0;

let failures = 0;
for (const run of runs) {
  const page = await context.newPage();
  const lines = [];
  page.on('console', (message) => lines.push(message.text()));
  page.on('pageerror', (error) => lines.push(`pageerror: ${error.message}`));
  const cdp = await context.newCDPSession(page);
  await cdp.send('Performance.enable');
  try {
    await page.goto(`${base}/?${run.query}`);
    await page.bringToFront();
    await page.waitForFunction(() => globalThis.__gpuxBench?.measuring === true || globalThis.__gpuxBench?.error, null, { timeout: timeoutMs, polling: 10 });
    const before = (await cdp.send('Performance.getMetrics')).metrics;
    await page.waitForFunction(() => globalThis.__gpuxBench?.done === true || globalThis.__gpuxBench?.error, null, { timeout: timeoutMs, polling: 10 });
    const after = (await cdp.send('Performance.getMetrics')).metrics;
    const error = await page.evaluate(() => globalThis.__gpuxBench.error);
    if (error) throw new Error(error);
    const report = JSON.parse(await page.evaluate(() => globalThis.__gpuxBench.report));
    const frames = report.frames;
    const task = metric(after, 'TaskDuration') - metric(before, 'TaskDuration');
    const script = metric(after, 'ScriptDuration') - metric(before, 'ScriptDuration');
    const wall = metric(after, 'Timestamp') - metric(before, 'Timestamp');
    // Chrome's main-thread task time over the measured frames: the page's CPU time.
    report.main_cpu_ms = { mean: (task * 1000) / frames, p50: null, p95: null, p99: null, max: null };
    report.process_cpu_ms_per_frame = (task * 1000) / frames;
    report.process_cpu_pct = (task / wall) * 100;
    report.script_ms_per_frame = (script * 1000) / frames;
    report.js_heap_mb = metric(after, 'JSHeapUsedSize') / (1024 * 1024);
    report.browser = browser.version();
    report.cross_origin_isolated = await page.evaluate(() => globalThis.__gpuxBench.crossOriginIsolated);
    fs.writeFileSync(run.out, JSON.stringify(report, null, 2));
    console.log(`ok ${run.query}`);
  } catch (error) {
    failures += 1;
    console.log(`FAIL ${run.query}: ${error.message}\n${lines.slice(-20).join('\n')}`);
  } finally {
    await page.close();
  }
}
await browser.close();
server.close();
process.exit(failures ? 1 : 0);
