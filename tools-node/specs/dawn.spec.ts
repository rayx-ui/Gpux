import { laneBScenes } from '../fixtures/catalog';
import { expect, test } from '../fixtures/gpux';

// A Dawn validation smoke: every lane-B scene and browser-only scene renders its first frames,
// which together use every render pipeline (quads, shadows, paths, underlines, monochrome and
// polychrome sprites, surfaces), without a single WebGPU validation error. The fixture fails on
// Dawn's validation warnings and on any uncaptured device error.

const scenes = [
  ...laneBScenes().map((scene) => scene.id),
  'web-html-element',
  'web-html-island',
  'web-svg-asset',
  'web-cursors',
  'web-lifecycle',
  'web-safe-area',
];

test('every scene renders without a WebGPU validation error @pr', async ({ gpux, page }) => {
  test.setTimeout(240_000);
  for (const scene of scenes) {
    await gpux.open(scene);
    await gpux.frames(3);
    const errors = await page.evaluate(
      () => (globalThis as unknown as { __gpuxWebGpuErrors?: string[] }).__gpuxWebGpuErrors ?? [],
    );
    expect(errors, `${scene} raised WebGPU errors`).toEqual([]);
  }
  expect(gpux.problems).toEqual([]);
});
