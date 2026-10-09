import type { Page } from '@playwright/test';
import { type Gpux, expect, test } from '../fixtures/gpux';

// Glyphs the bundled fonts lack: whole emoji graphemes rasterize once through a Canvas 2D
// fallback (configured for frequent readback) and are reused from the atlas, while bundled
// glyphs, CJK under the default policy and unsupported scripts never touch the canvas.
// Migrated from gpui_web's hello_web `test_canvas_fallback.mjs`.

interface CanvasCall {
  type: string;
  text: string;
  willReadFrequently: boolean;
}

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    const calls: CanvasCall[] = [];
    (window as unknown as { __canvasCalls: CanvasCall[] }).__canvasCalls = calls;
    for (const name of ['measureText', 'fillText'] as const) {
      const prototype = OffscreenCanvasRenderingContext2D.prototype as unknown as Record<
        string,
        (...args: unknown[]) => unknown
      >;
      const original = prototype[name];
      prototype[name] = function (this: OffscreenCanvasRenderingContext2D, ...args: unknown[]) {
        calls.push({
          type: name,
          text: String(args[0]),
          willReadFrequently: this.getContextAttributes().willReadFrequently === true,
        });
        return original.apply(this, args);
      };
    }
  });
});

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { __canvasCalls: CanvasCall[] }).__canvasCalls);
const rasterCount = async (page: Page, text: string) =>
  (await calls(page)).filter((call) => call.type === 'fillText' && call.text === text).length;

/** Replaces the field's text and waits until it shows `text`. */
async function replaceText(gpux: Gpux, text: string) {
  await gpux.page.keyboard.press(process.platform === 'darwin' ? 'Meta+a' : 'Control+a');
  await gpux.page.keyboard.insertText(text);
  await gpux.locate(`role=textbox name=Name value="${text}"`);
  await gpux.frames(2);
}

test('emoji rasterize once through the canvas fallback @pr', async ({ gpux, page }) => {
  await gpux.open('editable-text');
  await gpux.click('role=textbox name=Name');

  await replaceText(gpux, 'Hello ©');
  expect(await calls(page), 'bundled glyphs do not use the canvas').toEqual([]);

  for (const emoji of ['😀', '🕸']) {
    await replaceText(gpux, emoji.repeat(3));
    expect(await rasterCount(page, emoji), `repeated ${emoji} share one atlas image`).toBe(1);
    await page.keyboard.press('ArrowLeft');
    await page.keyboard.press('ArrowRight');
    await gpux.frames(2);
    expect(await rasterCount(page, emoji), `caret movement does not rerasterize ${emoji}`).toBe(1);
  }

  const text = 'A中文 が が 각 각 🕸 🕸️ 🕸︎ 🕸 😀 ❤️ 👍🏽 🇯🇵 1️⃣ 👩‍💻 👨‍👩‍👧‍👦Z';
  await replaceText(gpux, text);
  for (const grapheme of ['🕸', '🕸️', '🕸︎', '❤️', '👍🏽', '🇯🇵', '1️⃣', '👩‍💻', '👨‍👩‍👧‍👦']) {
    expect(await rasterCount(page, grapheme), `whole-grapheme raster of ${grapheme}`).toBeGreaterThan(0);
  }
  for (const grapheme of ['中', '文', 'が', 'が', '각', '각']) {
    expect(await rasterCount(page, grapheme), `the default policy keeps ${grapheme} native`).toBe(0);
  }
  expect((await calls(page)).every((call) => call.willReadFrequently)).toBe(true);

  await page.keyboard.press('Backspace');
  await page.keyboard.press('Backspace');
  await gpux.locate(`role=textbox name=Name value="${text.slice(0, -'👨‍👩‍👧‍👦Z'.length)}"`);
});

test('unsupported scripts stay native @pr', async ({ gpux, page }) => {
  await gpux.open('editable-text');
  await gpux.click('role=textbox name=Name');
  await replaceText(gpux, 'ش');
  expect(await calls(page)).toEqual([]);
});

test('missing glyphs are reported once per grapheme @pr', async ({ gpux }) => {
  await gpux.open('paint-borders');
  // Throws with the failed check; resolves without a value when every check holds.
  await gpux.callExport('missingGlyphNotifications');
});
