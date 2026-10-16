import { runScript } from '../fixtures/driver';
import { expect, test } from '../fixtures/gpux';

// Assets fetched over HTTP: an SVG named by `Svg::external_path` loads from the server and paints
// tinted by the text color.

test('an external SVG is fetched and painted @pr', async ({ gpux, page }) => {
  const fetched = page.waitForResponse((response) => response.url().endsWith('/assets/conformance.svg'));
  await gpux.open('web-svg-asset');
  expect((await fetched).status()).toBe(200);
  expect((await runScript(gpux)).ran).toBe(3);
});

test.describe('at device pixel ratio 2', () => {
  test.use({ deviceScaleFactor: 2 });

  test('an external SVG paints at device resolution @dpr2', async ({ gpux }) => {
    await gpux.open('web-svg-asset');
    expect((await runScript(gpux)).ran).toBe(3);
  });
});
