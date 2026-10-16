import { expect, test } from '../fixtures/gpux';

// Window composition in the browser (`Window::enable_window_composition`): on WebGPU, GPUI's
// overlays draw on a canvas above the DOM overlay, so a popover covers a DOM element and takes the
// pointer where it drew, while the DOM element keeps it elsewhere. WebGL2 reports composition
// unsupported and keeps the single canvas below the DOM overlay.

const GREEN: [number, number, number, number] = [0, 255, 0, 255];
const BLUE: [number, number, number, number] = [0, 0, 255, 255];
/** Inside the popover, which sits over the DOM block. */
const ON_POPOVER = { x: 100, y: 80 };
/** On the DOM block, beside the popover. */
const BESIDE = { x: 30, y: 50 };
/** On GPUI's own content, clear of the DOM block and the popover. */
const ON_CANVAS = { x: 220, y: 140 };

test('a popover draws over a DOM element on WebGPU @pr', async ({ gpux }) => {
  await gpux.open('web-composition', { backend: 'webgpu' });
  await gpux.locate('role=status value="composition on clicks 0"');
  await gpux.expectColor(ON_POPOVER.x, ON_POPOVER.y, GREEN);
  await gpux.expectColor(BESIDE.x, BESIDE.y, BLUE);
});

test('the popover takes the pointer where it drew and the DOM element beside it @pr', async ({
  gpux,
  page,
}) => {
  await gpux.open('web-composition', { backend: 'webgpu' });
  await gpux.locate('role=status value="composition on clicks 0"');
  const block = page.locator('#gpux-dom-block');
  await expect(block).toHaveCount(1);
  // Once the overlay's first frame is drawn, its shield takes the pointer over the popover.
  await expect(page.locator('[data-gpux-composition-shield]')).toBeVisible();

  await page.mouse.click(ON_POPOVER.x, ON_POPOVER.y);
  await gpux.locate('role=status value="composition on clicks 1"');
  await expect(block).not.toHaveAttribute('data-clicks', /.*/);

  await page.mouse.click(BESIDE.x, BESIDE.y);
  await expect(block).toHaveAttribute('data-clicks', '1');
  await gpux.locate('role=status value="composition on clicks 1"');
});

test('moving between the canvas and the popover stays inside the window @pr', async ({
  gpux,
  page,
}) => {
  await gpux.open('web-composition', { backend: 'webgpu' });
  await gpux.locate('role=status value="composition on clicks 0"');
  // The overlay takes the pointer once its first frame is drawn; until then the DOM block
  // under the popover does, and moving onto a DOM-owned element does leave GPUI's content.
  await gpux.expectColor(ON_POPOVER.x, ON_POPOVER.y, GREEN);
  await expect(page.locator('[data-gpux-composition-shield]')).toBeVisible();
  await page.mouse.move(ON_CANVAS.x, ON_CANVAS.y);
  await page.mouse.move(ON_POPOVER.x, ON_POPOVER.y);
  await page.mouse.move(ON_CANVAS.x, ON_CANVAS.y);
  await page.mouse.click(ON_POPOVER.x, ON_POPOVER.y);
  await gpux.locate('role=status value="composition on clicks 1"');
  await gpux.locate('role=status value="exits 0"');
});

test('forced WebGL2 reports composition unsupported and keeps the DOM overlay on top @pr', async ({
  gpux,
}) => {
  await gpux.open('web-composition', { backend: 'webgl' });
  await gpux.locate('role=status value="composition unsupported clicks 0"');
  // Without composition the DOM overlay stays above the canvas, popover included.
  await gpux.expectColor(ON_POPOVER.x, ON_POPOVER.y, BLUE);
});
