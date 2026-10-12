import type { Page } from '@playwright/test';
import { SOFTWARE_PROFILES } from '../fixtures/catalog';
import { expect, test, type Backend, type Bounds, type Gpux } from '../fixtures/gpux';

// Scroll layers in the browser (fast mode's `scroll_layers`): a scrolled div and a list draw from
// tiles the renderer rasterized once, on WebGPU and on the WebGL2 transport. The web host's
// `?layers=1|0` switch turns every fast-mode mechanism on or off, so a page with layers is
// compared with the same page without them. `__gpuxFastStats` (published under `diagnostics`)
// says which frames came from tiles.

// The pages are small and so is the window: a software renderer (the CI profiles) draws every
// frame of the canvas, and the canvas fills the viewport.
test.use({ viewport: { width: 440, height: 340 } });

interface FastStats {
  frame: Record<string, number>;
  run: Record<string, number>;
  /** What the renderer did with tiles, and the bytes of tiles a window may keep. */
  tiles?: { held: number; rasterized: number; released: number; budget: number };
}

/** The browser's budget of tiles per window: 32 MiB (64 MiB on desktop). */
const BROWSER_TILE_BUDGET = 32 << 20;

const BACKENDS: Backend[] = ['webgpu', 'webgl'];
const NAMES: Record<Backend, string> = { webgpu: 'WebGPU', webgl: 'WebGL2' };

/** The fast-mode counters of the last frame and of the run so far. */
async function stats(page: Page): Promise<FastStats> {
  const published = await page.evaluate(
    () => (globalThis as unknown as { __gpuxFastStats?: FastStats }).__gpuxFastStats ?? null,
  );
  expect(published, 'the host publishes __gpuxFastStats under diagnostics').not.toBeNull();
  return published!;
}

/** The status line's text. */
async function statusOf(gpux: Gpux, query: string): Promise<Bounds> {
  return gpux.locate(query);
}

/** Turns the wheel `steps` times by `delta` over `at`, one presented frame each. */
async function scroll(gpux: Gpux, at: { x: number; y: number }, steps: number, delta: number) {
  await gpux.page.mouse.move(at.x, at.y);
  for (let step = 0; step < steps; step++) {
    await gpux.page.mouse.wheel(0, delta);
    await gpux.frames(2);
  }
}

/**
 * Scrolls the div and the list of `web-scroll-layers` the same way and returns what shows. The
 * browser divides a synthesized wheel delta by the device pixel ratio, so the delta is scaled
 * back and the page scrolls 40 CSS pixels a step at any ratio.
 */
async function scrolled(gpux: Gpux) {
  const ratio = await gpux.page.evaluate(() => globalThis.devicePixelRatio);
  const div = await gpux.center('id=scroller');
  await scroll(gpux, div, 8, 40 * ratio);
  await statusOf(gpux, 'role=status value="div 320 list 0"');
  const list = await gpux.center('id=rows');
  await scroll(gpux, list, 6, 40 * ratio);
  await expect
    .poll(async () => (await gpux.find('role=status value="div 320 list 3"')) !== null, {
      message: 'the list scrolled to its fourth row',
    })
    .toBe(true);
  await gpux.frames(3);
  return gpux.capture({ x: 0, y: 0, width: 440, height: 340 });
}

for (const backend of BACKENDS) {
  test(`scroll-layers: a scrolled div and list composite their tiles on ${NAMES[backend]} @pr`, async ({
    gpux,
    page,
  }) => {
    await gpux.open('web-scroll-layers', { backend, params: { layers: '1' } });
    await gpux.locate('role=status value="div 0 list 0"');
    const before = (await stats(page)).run;
    await scrolled(gpux);
    const published = await stats(page);
    const after = published.run;
    expect(published.tiles?.budget, 'the browser tile budget applies').toBe(BROWSER_TILE_BUDGET);
    expect(published.tiles?.held, 'tiles the renderer holds').toBeGreaterThan(0);
    expect(
      after.tiles_rasterized - before.tiles_rasterized,
      'tiles rasterized while the page scrolled',
    ).toBeGreaterThan(0);
    expect(
      after.layers_composited - before.layers_composited,
      'scroll-container frames drawn from tiles',
    ).toBeGreaterThanOrEqual(8);
    expect(
      after.tiles_composited - before.tiles_composited,
      'tiles composited into frames',
    ).toBeGreaterThan(0);
  });

  test(`scroll-layers: a page with layers looks like the page without them on ${NAMES[backend]} @pr`, async ({
    gpux,
    page,
  }) => {
    await gpux.open('web-scroll-layers', { backend, params: { layers: '1' } });
    await gpux.locate('role=status value="div 0 list 0"');
    const withLayers = await scrolled(gpux);
    expect((await stats(page)).run.layers_composited, 'the page composited').toBeGreaterThan(0);

    await gpux.open('web-scroll-layers', { backend, params: { layers: '0' } });
    await gpux.locate('role=status value="div 0 list 0"');
    const without = await scrolled(gpux);
    const off = (await stats(page)).run;
    expect(off.layers_composited, 'the layers-off page never composites').toBe(0);
    expect(off.tiles_rasterized, 'the layers-off page rasterizes no tiles').toBe(0);

    expect([withLayers.width, withLayers.height]).toEqual([without.width, without.height]);
    const differing = withLayers.data.reduce(
      (count, byte, index) => count + (byte === without.data[index] ? 0 : 1),
      0,
    );
    expect(differing, 'bytes that differ between the page with layers and without').toBe(0);
  });

  for (const [scene, content] of [
    ['web-scroll-layers-html', 'a DOM element'],
    ['web-scroll-layers-island', 'an HTML island'],
  ] as const) {
    test(`scroll-layers: a scroller holding ${content} is drawn without tiles on ${NAMES[backend]} @pr`, async ({
      gpux,
      page,
    }) => {
      await gpux.open(scene, { backend, params: { layers: '1' } });
      await gpux.locate('role=status value="div 0"');
      const div = await gpux.center('id=scroller');
      await scroll(gpux, div, 8, 40);
      await gpux.locate('role=status value="div 320"');
      const run = (await stats(page)).run;
      expect(run.layers_composited, 'no frame came from tiles').toBe(0);
      expect(run.tiles_rasterized, 'no tile was rasterized').toBe(0);
      expect(run['layer_bypasses.ineligible'], 'the scroller was found ineligible').toBeGreaterThan(0);
    });
  }
}

test.describe('at device pixel ratio 2', () => {
  test.use({ deviceScaleFactor: 2 });
  for (const backend of BACKENDS) {
    test(`scroll-layers: a page with layers looks like the page without them at DPR 2 on ${NAMES[backend]} @dpr2`, async ({
      gpux,
      page,
    }) => {
      await gpux.open('web-scroll-layers', { backend, params: { layers: '1' } });
      await gpux.locate('role=status value="div 0 list 0"');
      const withLayers = await scrolled(gpux);
      expect((await stats(page)).run.layers_composited, 'the page composited').toBeGreaterThan(0);

      await gpux.open('web-scroll-layers', { backend, params: { layers: '0' } });
      await gpux.locate('role=status value="div 0 list 0"');
      const without = await scrolled(gpux);
      const differing = withLayers.data.reduce(
        (count, byte, index) => count + (byte === without.data[index] ? 0 : 1),
        0,
      );
      expect(differing, 'bytes that differ between the page with layers and without').toBe(0);
    });
  }
});

/** The 60 Hz frame budget, in milliseconds. */
const FRAME_BUDGET_MS = 1000 / 60;

/**
 * Wheels `steps` times by `delta` over the element `target` of `web-scroll-layers` and returns
 * how long gpux's frame work (layout, paint and submission in its animation-frame callback) took
 * in each frame, and how many frames came from tiles.
 */
async function frameWork(gpux: Gpux, page: Page, target: string) {
  const at = await gpux.center(target);
  await page.mouse.move(at.x, at.y);
  // gpux requests every frame with the same function, which tells its callbacks from the spec's.
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
  const before = (await stats(page)).run.layers_composited;
  for (let step = 0; step < 60; step++) {
    await page.mouse.wheel(0, 20);
    await gpux.frames(1);
  }
  const composited = (await stats(page)).run.layers_composited - before;
  const durations = await page.evaluate(() => {
    const work = (globalThis as unknown as { __gpuxFrameWork: Map<unknown, number[]> }).__gpuxFrameWork;
    return [...work.values()].reduce((most, next) => (next.length > most.length ? next : most), []);
  });
  const sorted = [...durations].sort((a, b) => a - b);
  return {
    frames: sorted.length,
    composited,
    p95: sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95))] ?? 0,
    max: sorted[sorted.length - 1] ?? 0,
  };
}

// The same page scrolled with every mechanism and with none, on both backends: what the layers
// save per frame is what the second line of each pair says against the first. Lane G copies the
// `gpux-timing:` lines into its timing summary.
for (const backend of BACKENDS) {
  for (const [scene, target] of [
    ['cards in a scrolling div', 'id=scroller'],
    ['rows of a list', 'id=rows'],
  ] as const) {
    test(`@timing scrolling ${scene} keeps frame work within the 60 Hz budget with layers on ${NAMES[backend]}`, async ({
      gpux,
      page,
    }, testInfo) => {
      test.skip(SOFTWARE_PROFILES.includes(testInfo.project.name), 'software renderers make timing meaningless');
      const measured: Record<string, Awaited<ReturnType<typeof frameWork>>> = {};
      for (const layers of ['1', '0']) {
        await gpux.open('web-scroll-layers', { backend, params: { layers } });
        await gpux.locate('role=status value="div 0 list 0"');
        measured[layers] = await frameWork(gpux, page, target);
        console.log(
          `gpux-timing: scroll layers ${scene} on ${NAMES[backend]} with layers ${layers === '1' ? 'on' : 'off'}: frame work p95 ${measured[layers].p95.toFixed(2)} ms, max ${measured[layers].max.toFixed(2)} ms over ${measured[layers].frames} frames, ${measured[layers].composited} from tiles`,
        );
      }
      expect(measured['1'].frames, 'frames recorded while scrolling').toBeGreaterThanOrEqual(30);
      expect(measured['1'].composited, 'frames drawn from tiles while scrolling').toBeGreaterThanOrEqual(30);
      expect(measured['0'].composited, 'frames drawn from tiles with layers off').toBe(0);
      expect(measured['1'].p95, 'p95 frame work while scrolling from tiles (ms)').toBeLessThan(FRAME_BUDGET_MS);
    });
  }
}
