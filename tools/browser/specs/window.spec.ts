import { expect, test } from '../fixtures/gpux';

// The browser window: its frame loop runs while it is open, and closing it removes the canvas
// and stops the loop.

/** Counts the animation frames the page requests over `ms`. */
async function framesRequested(page: import('@playwright/test').Page, ms: number): Promise<number> {
  return page.evaluate(async (ms) => {
    let requested = 0;
    const original = window.requestAnimationFrame;
    window.requestAnimationFrame = (callback) => {
      requested += 1;
      return original.call(window, callback);
    };
    await new Promise((resolve) => setTimeout(resolve, ms));
    window.requestAnimationFrame = original;
    return requested;
  }, ms);
}

/** Counts the animation frames the page requests from now on, in `__gpuxFramesRequested`. */
async function countFrames(page: import('@playwright/test').Page): Promise<void> {
  await page.evaluate(() => {
    const counter = globalThis as unknown as { __gpuxFramesRequested: number };
    counter.__gpuxFramesRequested = 0;
    const original = window.requestAnimationFrame;
    window.requestAnimationFrame = (callback) => {
      counter.__gpuxFramesRequested += 1;
      return original.call(window, callback);
    };
  });
}

test('an open window keeps its frame loop running @pr', async ({ gpux, page }) => {
  await gpux.open('paint-borders');
  await countFrames(page);
  // Idle, the loop still requests frame after frame, however slowly a software renderer draws
  // them: the check waits for frames rather than counting them over a fixed time.
  await expect
    .poll(
      () =>
        page.evaluate(
          () => (globalThis as unknown as { __gpuxFramesRequested: number }).__gpuxFramesRequested,
        ),
      { timeout: 60_000 },
    )
    .toBeGreaterThanOrEqual(3);
});

test('closing the window removes its canvas and stops its frame loop @pr', async ({ gpux, page }) => {
  await gpux.open('paint-borders');
  expect(await page.locator('canvas').count()).toBe(1);
  expect(await page.evaluate(() => (globalThis.__gpuxConformance as unknown as { close(): boolean }).close())).toBe(true);
  await expect.poll(() => page.locator('canvas').count()).toBe(0);
  expect(await framesRequested(page, 500)).toBe(0);
});
