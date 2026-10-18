import { expect, test } from '../fixtures/gpux';

// The system clipboard: copy writes gpui's selection, paste inserts the clipboard text, and
// without the Clipboard API copy still reaches the system clipboard through `execCommand` while
// paste falls back to gpui's own clipboard.

test.use({ permissions: ['clipboard-read', 'clipboard-write'] });

/** The platform's text-editing modifier: Meta on macOS, Control elsewhere. */
const primary = process.platform === 'darwin' ? 'Meta' : 'Control';

test('copy puts the selection on the clipboard @pr', async ({ gpux, page }) => {
  await gpux.open('editable-text');
  await gpux.click('role=textbox name=Name');
  await page.keyboard.insertText('copied text');
  await gpux.locate('role=textbox name=Name value="copied text"');
  await page.keyboard.press(`${primary}+a`);
  await page.keyboard.press(`${primary}+c`);
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('copied text');
});

test('paste inserts the clipboard text @pr', async ({ gpux, page }) => {
  await gpux.open('editable-text');
  await page.evaluate(() => navigator.clipboard.writeText('pasted text'));
  await gpux.click('role=textbox name=Name');
  await page.keyboard.press(`${primary}+v`);
  await gpux.locate('role=textbox name=Name value="pasted text"');
});

/** A point inside `editable-rich-attributes`' field, right of its text. */
const RICH_FIELD = { x: 200, y: 30 };

test('clipboard: html copy writes text/html beside the text @pr', async ({ gpux, page }) => {
  await gpux.open('editable-rich-attributes');
  await page.mouse.click(RICH_FIELD.x, RICH_FIELD.y);
  await page.keyboard.press(`${primary}+a`);
  await page.keyboard.press(`${primary}+c`);
  await expect
    .poll(() => page.evaluate(() => navigator.clipboard.readText()))
    .toBe('HHHHHHHHHHHH');
  const html = await page.evaluate(() =>
    navigator.clipboard.read().then(async (items) => {
      const item = items.find((item) => item.types.includes('text/html'));
      return item ? (await item.getType('text/html')).text() : null;
    }),
  );
  expect(html).toContain('data-gpux');
  expect(html).toContain('font-size:28px');
});

test('clipboard: html paste inserts the formatting of text/html @pr', async ({ gpux, page }) => {
  await gpux.open('editable-rich-attributes');
  await gpux.locate('role=status value="bold off italic off"');
  await page.evaluate(() =>
    navigator.clipboard.write([
      new ClipboardItem({
        'text/plain': new Blob(['HH'], { type: 'text/plain' }),
        'text/html': new Blob(['<b>HH</b>'], { type: 'text/html' }),
      }),
    ]),
  );
  await page.mouse.click(RICH_FIELD.x, RICH_FIELD.y);
  await page.keyboard.press('Home');
  await page.keyboard.press(`${primary}+v`);
  // The pasted characters are the first two now, bold from the HTML.
  await gpux.locate('role=status value="bold on italic off"');
});

test.describe('without the Clipboard API', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => {
      (window as unknown as { __clipboard: Clipboard }).__clipboard = navigator.clipboard;
      Object.defineProperty(Navigator.prototype, 'clipboard', { get: () => undefined, configurable: true });
    });
  });

  test('copy reaches the system clipboard through execCommand @pr', async ({ gpux, page }) => {
    await gpux.open('editable-text');
    await gpux.click('role=textbox name=Name');
    await page.keyboard.insertText('fallback copy');
    await gpux.locate('role=textbox name=Name value="fallback copy"');
    await page.keyboard.press(`${primary}+a`);
    await page.keyboard.press(`${primary}+c`);
    await expect
      .poll(() => page.evaluate(() => (window as unknown as { __clipboard: Clipboard }).__clipboard.readText()))
      .toBe('fallback copy');
  });

  test('clipboard: html copy through execCommand writes text/html @pr', async ({ gpux, page }) => {
    await gpux.open('editable-rich-attributes');
    await page.mouse.click(RICH_FIELD.x, RICH_FIELD.y);
    await page.keyboard.press(`${primary}+a`);
    await page.keyboard.press(`${primary}+c`);
    await expect
      .poll(() =>
        page.evaluate(() =>
          (window as unknown as { __clipboard: Clipboard }).__clipboard.read().then(async (items) => {
            const item = items.find((item) => item.types.includes('text/html'));
            return item ? (await item.getType('text/html')).text() : '';
          }),
        ),
      )
      .toContain('font-size:28px');
  });

  test('paste inserts what the app copied @pr', async ({ gpux, page }) => {
    await gpux.open('editable-text');
    await gpux.click('role=textbox name=Name');
    await page.keyboard.insertText('abc');
    await page.keyboard.press(`${primary}+a`);
    await page.keyboard.press(`${primary}+c`);
    await page.keyboard.press('End');
    await page.keyboard.press(`${primary}+v`);
    await gpux.locate('role=textbox name=Name value="abcabc"');
  });
});
