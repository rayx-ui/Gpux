import { expect, test } from '../fixtures/gpux';

// The renderer backend: WebGPU first, WebGL2 when forced or when WebGPU is unavailable.

test('forcing WebGL2 renders through WebGL2 @pr', async ({ gpux }) => {
  await gpux.open('paint-borders', { backend: 'webgl' });
  expect(await gpux.expectFirstFrameProbes()).toBeGreaterThan(0);
});

test('without navigator.gpu the app falls back to WebGL2 @pr', async ({ gpux, page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, 'gpu', { get: () => undefined, configurable: true });
  });
  await gpux.open('paint-borders', { expectBackend: 'webgl' });
  expect(await gpux.expectFirstFrameProbes()).toBeGreaterThan(0);
});

test('a WebGPU adapter request that finds none falls back to WebGL2 @pr', async ({ gpux, page }) => {
  await page.addInitScript(() => {
    const gpu = (navigator as Navigator & { gpu?: { requestAdapter: () => Promise<null> } }).gpu;
    if (gpu) gpu.requestAdapter = async () => null;
  });
  await gpux.open('paint-borders', { expectBackend: 'webgl' });
  expect(await gpux.expectFirstFrameProbes()).toBeGreaterThan(0);
});

test('forcing WebGPU renders through WebGPU @pr', async ({ gpux }) => {
  await gpux.open('paint-borders', { backend: 'webgpu' });
  expect(await gpux.expectFirstFrameProbes()).toBeGreaterThan(0);
});
