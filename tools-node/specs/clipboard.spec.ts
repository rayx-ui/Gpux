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
