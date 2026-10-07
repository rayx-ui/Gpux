import { expect, test } from '../fixtures/gpux';

// Safe-area insets (CSS `env(safe-area-inset-*)`) shrink the window's fully visible bounds.

test('safe-area insets shrink the fully visible bounds @pr', async ({ gpux, page }) => {
  await gpux.open('web-safe-area');
  await gpux.locate('role=status value="visible 0 0 1280 720"');
  const session = await page.context().newCDPSession(page);
  await session.send('Emulation.setSafeAreaInsetsOverride', {
    insets: { top: 20, left: 10, bottom: 40, right: 30 },
  });
  await expect
    .poll(() => page.evaluate(() => globalThis.__gpuxConformance!.locate('role=status value="visible 10 20 1240 660"')))
    .not.toBeNull();
  await session.send('Emulation.setSafeAreaInsetsOverride', { insets: {} });
  await gpux.locate('role=status value="visible 0 0 1280 720"');
});
