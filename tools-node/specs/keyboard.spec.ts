import { expect, test } from '../fixtures/gpux';

// Trusted keyboard input: keys gpui binds reach its keymaps instead of the browser, focus stays
// in the page while Tab moves it, a held key repeats, and AltGr characters type text instead of
// matching Control+Alt bindings.

test('a key binding gpui handles does not reach the browser @pr', async ({ gpux, page }) => {
  await gpux.open('actions-keymaps');
  let navigations = 0;
  page.on('framenavigated', () => (navigations += 1));
  await page.evaluate(() => {
    (window as unknown as { __keys: [string, boolean][] }).__keys = [];
    window.addEventListener('keydown', (event) =>
      (window as unknown as { __keys: [string, boolean][] }).__keys.push([event.key, event.defaultPrevented]),
    );
  });
  await gpux.click('id=counter');
  await page.keyboard.press('Control+i');
  await gpux.locate('role=status value="count 1"');
  // Control+R reloads a page unless the app consumes it; the scene binds it to Reset.
  await page.keyboard.press('Control+r');
  await gpux.locate('role=status value="count 0"');
  expect(navigations).toBe(0);
  const keys = await page.evaluate(() => (window as unknown as { __keys: [string, boolean][] }).__keys);
  expect(keys.filter(([key]) => key === 'r')).toEqual([['r', true]]);
});

test('Tab moves focus inside the scene without leaving the page @pr', async ({ gpux, page }) => {
  await gpux.open('focus-tab-order');
  await gpux.click('id=first');
  await page.keyboard.press('Tab');
  await gpux.frames();
  expect(await page.evaluate(() => document.activeElement?.tagName)).not.toBe('BODY');
  expect(await page.evaluate(() => document.hasFocus())).toBe(true);
  await expect
    .poll(() => gpux.page.evaluate(() => globalThis.__gpuxConformance!.locate('id=second')))
    .not.toBeNull();
});

test('a held key repeats @pr', async ({ gpux, page }) => {
  await gpux.open('editable-text');
  await gpux.click('role=textbox name=Name');
  // Each further keydown before the keyup is an auto-repeat.
  await page.keyboard.down('x');
  await page.keyboard.down('x');
  await page.keyboard.down('x');
  await page.keyboard.up('x');
  await gpux.locate('role=textbox name=Name value="xxx"');
});

test('an AltGr character types text @pr', async ({ gpux, page }) => {
  await gpux.open('editable-text');
  await gpux.click('role=textbox name=Name');
  const session = await page.context().newCDPSession(page);
  // AltGr+Q on a German layout: Windows reports AltGr as Control+Alt.
  const altGr = { modifiers: 1 | 2, key: '@', code: 'KeyQ', windowsVirtualKeyCode: 81 };
  await session.send('Input.dispatchKeyEvent', { type: 'keyDown', text: '@', unmodifiedText: 'q', ...altGr });
  await session.send('Input.dispatchKeyEvent', { type: 'keyUp', ...altGr });
  await gpux.locate('role=textbox name=Name value="@"');
});
