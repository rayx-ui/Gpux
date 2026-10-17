import { expect, test } from '../fixtures/gpux';

// The smallest proof that lane B works on this host: WebGPU starts in an isolated page, the
// canvas shows the scene, and trusted input at located bounds reaches it.

test('WebGPU starts in a cross-origin isolated page @canary', async ({ gpux }) => {
  await gpux.open('paint-borders');
  const threads = await gpux.page.evaluate(() => globalThis.__gpuxThreadDiagnostics);
  expect(threads).toMatchObject({ crossOriginIsolated: true, sharedArrayBuffer: true });
  expect(await gpux.page.evaluate(() => typeof navigator.gpu)).toBe('object');
});

test('the canvas shows the scene @canary', async ({ gpux }) => {
  await gpux.open('layout-flex');
  expect(await gpux.distinctColors({ x: 0, y: 0, width: 320, height: 80 })).toBeGreaterThan(2);
  expect(await gpux.expectFirstFrameProbes()).toBeGreaterThan(0);
});

test('a located click reaches the scene @canary', async ({ gpux }) => {
  await gpux.open('overlay-anchored');
  await gpux.expectMissing('id=popover');
  await gpux.click('id=toggle');
  expect(await gpux.locate('id=popover')).toEqual({ x: 20, y: 48, width: 100, height: 50 });
  await gpux.expectColor(70, 70, [0, 0, 255, 255]);
  await gpux.click('id=toggle');
  await gpux.expectMissing('id=popover');
});
