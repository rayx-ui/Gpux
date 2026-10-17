import { runScript } from '../fixtures/driver';
import { expect, test } from '../fixtures/gpux';

// IME composition through the browser's input method (CDP `Input.imeSetComposition`): marked
// text, commit and cancel reach the focused editable text through gpui_web's input mirror.

test('composition goes through the input mirror @pr', async ({ gpux, page }) => {
  await gpux.open('editable-ime');
  const result = await runScript(gpux);
  expect(result.ran).toBeGreaterThan(5);
  // The focused editable text keeps a DOM text input focused for the input method.
  expect(await page.evaluate(() => document.activeElement?.tagName)).toBe('TEXTAREA');
});

test('a composition commits into the field @pr', async ({ gpux, page }) => {
  await gpux.open('editable-text');
  await gpux.click('role=textbox name=Name');
  const session = await page.context().newCDPSession(page);
  await session.send('Input.imeSetComposition', { text: 'に', selectionStart: 1, selectionEnd: 1 });
  await gpux.frames();
  await session.send('Input.imeSetComposition', { text: 'にほん', selectionStart: 3, selectionEnd: 3 });
  await session.send('Input.insertText', { text: '日本' });
  await gpux.locate('role=textbox name=Name value="日本"');
});
