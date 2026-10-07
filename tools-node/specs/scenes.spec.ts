import { SOFTWARE_PROFILES, laneBScenes } from '../fixtures/catalog';
import { runScript } from '../fixtures/driver';
import { expect, test } from '../fixtures/gpux';

// Every lane-B conformance scene runs its own script with trusted browser input: CDP mouse,
// wheel, keyboard, touch and IME events reach the scene through gpui_web's DOM listeners, and the
// app checks the script's expectations and pixel steps against the real WebGPU frame (pixel
// steps read a screenshot of the page). Scenes that advance time assert what happens before and
// after a delay; software profiles render too slowly for that on the real clock and skip them.

const scenes = laneBScenes();

for (const scene of scenes) {
  test(`${scene.id} passes its script with trusted input @pr`, async ({ gpux }, testInfo) => {
    test.skip(
      scene.clock && SOFTWARE_PROFILES.includes(testInfo.project.name),
      'its script times delays on the real clock, which a software renderer is too slow for',
    );
    await gpux.open(scene.id);
    const result = await runScript(gpux);
    test.info().annotations.push({
      type: 'steps',
      description: `${result.ran} ran, ${result.skipped.length} skipped`,
    });
    expect(result.ran).toBeGreaterThan(0);
  });
}

test.describe('at device pixel ratio 2', () => {
  test.use({ deviceScaleFactor: 2 });
  for (const scene of scenes.filter((scene) => scene.scales.includes(2))) {
    test(`${scene.id} passes its script at DPR 2 @dpr2`, async ({ gpux }, testInfo) => {
      test.skip(
        scene.clock && SOFTWARE_PROFILES.includes(testInfo.project.name),
        'its script times delays on the real clock, which a software renderer is too slow for',
      );
      await gpux.open(scene.id);
      expect((await runScript(gpux)).ran).toBeGreaterThan(0);
    });
  }
});
