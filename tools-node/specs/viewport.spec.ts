import type { Page } from '@playwright/test';
import { expect, test } from '../fixtures/gpux';

// The canvas tracks the viewport: CSS size, device-pixel backing store, and relayout on resize
// through the ResizeObserver.

function canvasSize(page: Page) {
  return page.evaluate(() => {
    const canvas = document.querySelector('canvas')!;
    const rect = canvas.getBoundingClientRect();
    return {
      css: [rect.width, rect.height],
      backing: [canvas.width, canvas.height],
      viewport: [innerWidth, innerHeight],
      ratio: devicePixelRatio,
    };
  });
}

async function expectCoversViewport(page: Page) {
  await expect
    .poll(async () => {
      const size = await canvasSize(page);
      return {
        css: size.css,
        backing: size.backing,
      };
    })
    .toEqual(
      await (async () => {
        const size = await canvasSize(page);
        return {
          css: size.viewport,
          backing: size.viewport.map((length) => Math.round(length * size.ratio)),
        };
      })(),
    );
}

test('the canvas covers the viewport in device pixels @pr', async ({ gpux, page }) => {
  await gpux.open('layout-flex');
  await expectCoversViewport(page);
});

test('resizing the page resizes the canvas and keeps the layout @pr', async ({ gpux, page }) => {
  await gpux.open('layout-flex');
  const before = await gpux.locate('id=b');
  await page.setViewportSize({ width: 800, height: 500 });
  await expectCoversViewport(page);
  await gpux.frames(3);
  expect(await gpux.locate('id=b')).toEqual(before);
  await gpux.expectFirstFrameProbes();
});

test.describe('at device pixel ratio 2', () => {
  test.use({ deviceScaleFactor: 2 });

  test('the backing store doubles @dpr2', async ({ gpux, page }) => {
    await gpux.open('layout-flex');
    await expectCoversViewport(page);
    expect((await canvasSize(page)).backing).toEqual([2560, 1440]);
    await gpux.expectFirstFrameProbes();
  });
});
