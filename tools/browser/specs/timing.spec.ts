import type { Page } from '@playwright/test';
import { SOFTWARE_PROFILES } from '../fixtures/catalog';
import { expect, test } from '../fixtures/gpux';

// Lane G's timing thresholds: on a real GPU, gpux's frame work fits the frame budget. Only lane G
// runs @timing specs (`gpux-testkit lane g`); software profiles skip them, because timing on a
// software renderer or a shared runner measures the machine, not gpux.

/** The 60 Hz frame budget, in milliseconds. */
const FRAME_BUDGET_MS = 1000 / 60;

/**
 * Times gpux's animation-frame callbacks from now on: gpux lays out, paints and submits there.
 * gpux requests every frame with the same function, which tells its callbacks from the spec's own.
 */
async function recordFrameWork(page: Page): Promise<void> {
  await page.evaluate(() => {
    const durations = new Map<FrameRequestCallback, number[]>();
    (globalThis as unknown as { __gpuxFrameWork: typeof durations }).__gpuxFrameWork = durations;
    const original = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = (callback) =>
      original((time) => {
        const start = performance.now();
        try {
          callback(time);
        } finally {
          if (!durations.has(callback)) durations.set(callback, []);
          durations.get(callback)!.push(performance.now() - start);
        }
      });
  });
}

/** The durations of the callback requested most often: gpux's frame loop. */
async function frameWork(page: Page): Promise<number[]> {
  return page.evaluate(() => {
    const durations = (globalThis as unknown as { __gpuxFrameWork: Map<unknown, number[]> })
      .__gpuxFrameWork;
    return [...durations.values()].reduce((most, next) => (next.length > most.length ? next : most), []);
  });
}

function percentile(values: number[], fraction: number): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * fraction))];
}

test('@timing scrolling a virtualized list keeps frame work within the 60 Hz budget', async ({ gpux, page }, testInfo) => {
  test.skip(SOFTWARE_PROFILES.includes(testInfo.project.name), 'software renderers make timing meaningless');
  await gpux.open('list-uniform');
  const at = await gpux.center('text="Row 2"');
  await page.mouse.move(at.x, at.y);
  await recordFrameWork(page);
  // Every frame scrolls the list on, so every frame lays out new rows, paints and submits.
  for (let step = 0; step < 60; step++) {
    await page.mouse.wheel(0, 40);
    await gpux.frames(1);
  }
  // The list really scrolled, so the frames above did the work being timed.
  await gpux.expectMissing('text="Row 2"');
  const durations = await frameWork(page);
  expect(durations.length, 'frames recorded while scrolling').toBeGreaterThanOrEqual(30);
  const p95 = percentile(durations, 0.95);
  const max = Math.max(...durations);
  // The testkit copies `gpux-timing:` lines into lane G's timing summary.
  console.log(
    `gpux-timing: list-uniform scroll frame work p95 ${p95.toFixed(2)} ms, max ${max.toFixed(2)} ms over ${durations.length} frames`,
  );
  expect(p95, 'p95 frame work while scrolling (ms)').toBeLessThan(FRAME_BUDGET_MS);
});
