import type { Page } from '@playwright/test';
import { expect, test } from '../fixtures/gpux';

// HTML islands (`Window::paint_html_island`) on the browser's experimental HTML-in-canvas API.
// Where the browser has the API, the first frame with an island turns on the canvas's layout
// subtree and gives the island a DOM root inside the canvas; the `?gpux-html-island=1` and
// legacy `?rayx-html-island=1` opt-ins turn the subtree on early, before any island paints.
// Without the API no roots exist and the app's own fallback shows.

/** Whether the browser exposes the canvas half of the HTML-in-canvas API. */
function supportsHtmlInCanvas(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const canvas = HTMLCanvasElement.prototype as unknown as Record<string, unknown>;
    return 'requestPaint' in canvas && ('layoutSubtree' in canvas || 'layoutsubtree' in canvas);
  });
}

/** Whether WebGPU can copy an element's paint record into a texture. */
function supportsIslandUpload(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const queue = (globalThis as unknown as { GPUQueue?: { prototype: object } }).GPUQueue;
    return !!queue && 'copyElementImageToTexture' in queue.prototype;
  });
}

const layoutSubtree = (page: Page) =>
  page.evaluate(() => document.querySelector('canvas')!.hasAttribute('layoutsubtree'));

test('without islands or an opt-in the canvas keeps no layout subtree @pr', async ({ gpux, page }) => {
  await gpux.open('paint-borders');
  await gpux.frames(3);
  expect(await layoutSubtree(page)).toBe(false);
});

for (const optIn of ['gpux-html-island', 'rayx-html-island']) {
  test(`the ${optIn} opt-in turns the layout subtree on before any island @pr`, async ({ gpux, page }) => {
    await gpux.open('paint-borders', { params: { [optIn]: '1' } });
    test.skip(!(await supportsHtmlInCanvas(page)), 'this browser has no HTML-in-canvas API');
    expect(await layoutSubtree(page)).toBe(true);
  });
}

test('a painted island gets a DOM root inside the canvas @pr', async ({ gpux, page }) => {
  await gpux.open('web-html-island');
  if (!(await supportsHtmlInCanvas(page))) {
    // Without the API nothing is attached; the scene's own content still renders.
    await gpux.frames(3);
    expect(await page.locator('#gpux-island-block').count()).toBe(0);
    return;
  }
  await expect.poll(() => layoutSubtree(page)).toBe(true);
  await expect(page.locator('canvas #gpux-island-block')).toHaveCount(1);
  test.skip(!(await supportsIslandUpload(page)), 'this browser cannot upload element images to WebGPU');
  await gpux.expectColor(100, 65, [0, 0, 255, 255], 8);
});
