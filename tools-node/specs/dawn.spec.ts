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

// One test per scene, so the scenes spread over workers and shards and each gets the profile's
// timeout: rendering all of them in one test outlasts any fixed budget on software renderers.
for (const scene of scenes) {
  test(`${scene} renders without a WebGPU validation error @pr`, async ({ gpux, page }) => {
    await gpux.open(scene);
    await gpux.frames(3);
    const errors = await page.evaluate(
      () => (globalThis as unknown as { __gpuxWebGpuErrors?: string[] }).__gpuxWebGpuErrors ?? [],
    );
    expect(errors, `${scene} raised WebGPU errors`).toEqual([]);
    expect(gpux.problems).toEqual([]);
  });
}
