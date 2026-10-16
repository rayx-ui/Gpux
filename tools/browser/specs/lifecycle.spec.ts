import type { Page } from '@playwright/test';
import { test } from '../fixtures/gpux';

// Page lifecycle: the window is active while its page is visible and focused. Headless pages
// never leave the foreground, so the specs set the document's visibility and focus and fire the
// events gpui_web listens to (`visibilitychange`, `blur`, `focus`).

async function setVisibility(page: Page, state: 'hidden' | 'visible') {
  await page.evaluate((state) => {
    Object.defineProperty(document, 'visibilityState', { get: () => state, configurable: true });
    Object.defineProperty(document, 'hidden', { get: () => state === 'hidden', configurable: true });
    document.dispatchEvent(new Event('visibilitychange'));
  }, state);
}

async function setFocus(page: Page, focused: boolean) {
  await page.evaluate((focused) => {
    document.hasFocus = () => focused;
    window.dispatchEvent(new FocusEvent(focused ? 'focus' : 'blur'));
  }, focused);
}

test('hiding and showing the page deactivates and reactivates the window @pr', async ({ gpux, page }) => {
  await gpux.open('web-lifecycle');
  await gpux.locate('role=status value="active changes []"');
  await setVisibility(page, 'hidden');
  await gpux.locate('role=status value="inactive changes [inactive]"');
  await setVisibility(page, 'visible');
  await gpux.locate('role=status value="active changes [inactive active]"');
});

test('losing and regaining focus deactivates and reactivates the window @pr', async ({ gpux, page }) => {
  await gpux.open('web-lifecycle');
  await gpux.locate('role=status value="active changes []"');
  await setFocus(page, false);
  await gpux.locate('role=status value="inactive changes [inactive]"');
  await setFocus(page, true);
  await gpux.locate('role=status value="active changes [inactive active]"');
});
