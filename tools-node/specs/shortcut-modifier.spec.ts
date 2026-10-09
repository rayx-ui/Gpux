import { expect, test } from '../fixtures/gpux';

// Command is the secondary modifier in Apple browsers and Control elsewhere, whatever the WASM was
// compiled for: editable text, `secondary-` key bindings, the `os` key context, and how keystrokes
// are written and shown follow the browser's platform. Each test presents one platform to the
// app, so the spec gives the same answers on every host.

interface Platform {
  platform: string;
  userAgent: string;
  /** The secondary modifier, as Playwright names it. */
  secondary: 'Meta' | 'Control';
  /** The other one, which must not act as the secondary modifier. */
  other: 'Meta' | 'Control';
  /** The status `web-shortcut-modifier` shows before any key. */
  keystroke: string;
  /** Whether bindings for `os == macos` apply. */
  mac: boolean;
}

const PLATFORMS: Record<string, Platform> = {
  mac: {
    platform: 'MacIntel',
    userAgent:
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) ' +
      'Chrome/147.0.0.0 Safari/537.36',
    secondary: 'Meta',
    other: 'Control',
    keystroke: 'os macos written cmd-k shown ⌘K',
    mac: true,
  },
  windows: {
    platform: 'Win32',
    userAgent:
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) ' +
      'Chrome/147.0.0.0 Safari/537.36',
    secondary: 'Control',
    other: 'Meta',
    keystroke: 'os windows written ctrl-k shown ctrl-K',
    mac: false,
  },
};

for (const [name, os] of Object.entries(PLATFORMS)) {
  test.describe(`a ${name} browser`, () => {
    test.beforeEach(async ({ page }) => {
      await page.addInitScript(({ platform, userAgent }) => {
        Object.defineProperty(Navigator.prototype, 'platform', { get: () => platform });
        Object.defineProperty(Navigator.prototype, 'userAgent', { get: () => userAgent });
      }, os);
    });

    test(`editable text takes ${os.secondary} as the shortcut modifier @pr`, async ({ gpux, page }) => {
      await gpux.open('editable-text');
      await gpux.click('role=textbox name=Name');
      await page.keyboard.insertText('alpha beta');
      await gpux.locate('role=status value="selection 10..10 marked none"');

      await page.keyboard.press(`${os.other}+a`);
      await gpux.frames(2);
      await gpux.locate('role=status value="selection 10..10 marked none"');

      await page.keyboard.press(`${os.secondary}+a`);
      await gpux.locate('role=status value="selection 0..10 marked none"');

      await page.keyboard.press('End');
      await page.keyboard.press(`${os.secondary}+Backspace`);
      await gpux.locate('role=textbox name=Name value="alpha "');
    });

    test(`key bindings, the os context and keystroke names follow ${name} @pr`, async ({ gpux, page }) => {
      await gpux.open('web-shortcut-modifier');
      await gpux.locate(`role=status value="${os.keystroke} shortcuts 0 mac 0"`);
      await gpux.click('id=target');

      await page.keyboard.press(`${os.secondary}+k`);
      await gpux.locate(`role=status value="${os.keystroke} shortcuts 1 mac 0"`);
      await page.keyboard.press(`${os.other}+k`);
      await gpux.frames(2);
      await gpux.locate(`role=status value="${os.keystroke} shortcuts 1 mac 0"`);

      await page.keyboard.press('F2');
      await gpux.frames(2);
      await gpux.locate(`role=status value="${os.keystroke} shortcuts 1 mac ${os.mac ? 1 : 0}"`);
    });
  });
}
