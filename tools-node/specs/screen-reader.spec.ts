import type { Locator, Page } from '@playwright/test';
import { expect, test, type Bounds, type Gpux } from '../fixtures/gpux';

// Screen readers in the browser: gpui_web mirrors a window's AccessKit tree as DOM elements
// beside its canvas (role, name, states, values, relations, boxes), which Chromium's
// accessibility tree and Playwright's role queries read. The mirror starts from the hidden
// "Enable accessibility" button, or from the first frame with `?screen-reader=always`
// (`WebScreenReader::Always`). DOM clicks, focus and the ARIA keys of the mirror's widgets reach
// the scene as AccessKit actions; GPUI's focus moves DOM focus to the mirror.

const MIRROR = '[data-gpux-a11y-mirror]';

interface A11yHost {
  a11yTree(): string | null;
}

interface TreeJson {
  gpui_focus: string | null;
  nodes: Record<string, { aria: { role: string; label?: string } }>;
}

/** The mirror's root: its descendants are the screen-reader tree. */
function mirror(page: Page): Locator {
  return page.locator(MIRROR);
}

async function openMirrored(gpux: Gpux, scene = 'accessibility', params: Record<string, string> = {}) {
  await gpux.open(scene, { params: { 'screen-reader': 'always', ...params } });
  await expect(mirror(gpux.page)).toHaveCount(1);
}

function status(page: Page): Locator {
  return mirror(page).getByRole('status');
}

async function expectStatus(page: Page, text: string) {
  await expect(status(page)).toHaveText(text);
}

async function tree(page: Page): Promise<TreeJson> {
  const json = await page.evaluate(() =>
    (globalThis.__gpuxConformance as unknown as A11yHost).a11yTree(),
  );
  expect(json).not.toBeNull();
  return JSON.parse(json as string) as TreeJson;
}

/** How many action requests the mirror sent. */
async function actionsSent(page: Page): Promise<number> {
  return Number((await mirror(page).getAttribute('data-gpux-actions')) ?? '0');
}

function near(actual: Bounds, expected: Bounds) {
  for (const key of ['x', 'y', 'width', 'height'] as const) {
    expect(Math.abs(actual[key] - expected[key]), `${key}: ${actual[key]} vs ${expected[key]}`).toBeLessThan(1.5);
  }
}

test("screen-reader: tree mirrors the scene's roles, names, states and values @pr", async ({ gpux, page }) => {
  await openMirrored(gpux);
  const root = mirror(page);
  await expect(root.getByRole('button', { name: 'Activate' })).toHaveCount(1);
  const subscribe = root.getByRole('checkbox', { name: 'Subscribe' });
  await expect(subscribe).toHaveAttribute('aria-checked', 'false');
  const volume = root.getByRole('slider', { name: 'Volume' });
  await expect(volume).toHaveAttribute('aria-valuenow', '5');
  await expect(volume).toHaveAttribute('aria-valuemin', '0');
  await expect(volume).toHaveAttribute('aria-valuemax', '10');
  await expect(root.getByRole('textbox', { name: 'Title' })).toHaveCount(1);
  const email = root.getByRole('textbox', { name: 'Email' });
  await expect(email).toHaveAttribute('aria-required', 'true');
  await expect(email).toHaveText('ada@example.com');
  const fruits = root.getByRole('list', { name: 'Fruits' });
  await expect(fruits.getByRole('listitem')).toHaveCount(3);
  await expect(fruits.getByRole('listitem').nth(1)).toHaveText('Pear');
  // The status line is a live region.
  await expect(status(page)).toHaveAttribute('aria-live', 'polite');
  await expectStatus(page, 'clicks 0 subscribed false volume 5');
  // Chromium's own accessibility tree reads the same.
  const snapshot = await root.ariaSnapshot();
  expect(snapshot).toContain('button "Activate"');
  expect(snapshot).toContain('checkbox "Subscribe"');
  expect(snapshot).toContain('slider "Volume"');
});

test('screen-reader: tree relations point at the elements they name @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  const email = mirror(page).getByRole('textbox', { name: 'Email' });
  await expect(email).toHaveAccessibleDescription('We never share it');
  const labelledBy = (await email.getAttribute('aria-labelledby'))!;
  const describedBy = (await email.getAttribute('aria-describedby'))!;
  await expect(page.locator(`[id="${labelledBy}"]`)).toHaveText('Email');
  await expect(page.locator(`[id="${describedBy}"]`)).toHaveText('We never share it');
});

test('screen-reader: tree boxes sit on the painted controls @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  const root = mirror(page);
  for (const [locator, query] of [
    [root.getByRole('button', { name: 'Activate' }), 'role=button name=Activate'],
    [root.getByRole('checkbox', { name: 'Subscribe' }), 'role=checkbox name=Subscribe'],
    [root.getByRole('slider', { name: 'Volume' }), 'role=slider name=Volume'],
  ] as const) {
    const box = (await locator.boundingBox())!;
    near(box, await gpux.locate(query));
  }
  // The mirror never takes the pointer: a click at the button lands on the canvas.
  const button = await gpux.center('role=button name=Activate');
  const hit = await page.evaluate(
    ({ x, y }) => document.elementFromPoint(x, y)?.tagName,
    button,
  );
  expect(hit).toBe('CANVAS');
});

test('screen-reader: tree follows an update and keeps unchanged elements @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  const root = mirror(page);
  const activate = root.getByRole('button', { name: 'Activate' });
  await activate.evaluate((element) => ((element as unknown as { kept: boolean }).kept = true));
  await gpux.click('role=checkbox name=Subscribe');
  await expect(root.getByRole('checkbox', { name: 'Subscribe' })).toHaveAttribute('aria-checked', 'true');
  await expectStatus(page, 'clicks 0 subscribed true volume 5');
  expect(await activate.evaluate((element) => (element as unknown as { kept?: boolean }).kept)).toBe(true);
});

test('screen-reader: tree has no element for generic containers @pr', async ({ gpux, page }) => {
  // The host inspects the scene, so the tree holds generic containers for its ids.
  await openMirrored(gpux);
  const json = await tree(page);
  const roles = Object.values(json.nodes).map((node) => node.aria.role);
  expect(roles).toContain('GenericContainer');
  const withElements = roles.filter((role) => role !== 'GenericContainer' && role !== 'Window');
  await expect(page.locator(`${MIRROR} [data-gpux-node]`)).toHaveCount(withElements.length);
});

test('screen-reader: tree orders an html_element in with aria-owns @pr', async ({ gpux, page }) => {
  await openMirrored(gpux, 'web-html-element');
  const host = mirror(page).getByRole('group', { name: 'DOM content' });
  await expect(host).toHaveAttribute('aria-owns', 'gpux-html-element-gpux-dom-button');
  await expect(page.locator('#gpux-html-element-gpux-dom-button #gpux-dom-button')).toHaveCount(1);
  // Read in its place: inside the group, in Chromium's tree.
  await expect(host).toBeAttached();
  expect(await host.ariaSnapshot()).toContain('button "DOM"');
});

test('screen-reader: input a DOM click activates the button @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  await mirror(page)
    .getByRole('button', { name: 'Activate' })
    .evaluate((element) => (element as HTMLElement).click());
  await expectStatus(page, 'clicks 1 subscribed false volume 5');
  await expect(mirror(page)).toHaveAttribute('data-gpux-last-action', /^Click /);
});

test('screen-reader: input slider keys step its value @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  const volume = mirror(page).getByRole('slider', { name: 'Volume' });
  await volume.focus();
  await page.keyboard.press('ArrowRight');
  await expectStatus(page, 'clicks 0 subscribed false volume 6');
  await page.keyboard.press('ArrowUp');
  await expectStatus(page, 'clicks 0 subscribed false volume 7');
  await page.keyboard.press('ArrowLeft');
  await expectStatus(page, 'clicks 0 subscribed false volume 6');
  await expect(volume).toHaveAttribute('aria-valuenow', '6');
});

test('screen-reader: input DOM focus focuses the element in gpux @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  await mirror(page).getByRole('checkbox', { name: 'Subscribe' }).focus();
  await expect(mirror(page)).toHaveAttribute('data-gpux-last-action', /^Focus /);
  await expect
    .poll(async () => {
      const json = await tree(page);
      return json.gpui_focus ? json.nodes[json.gpui_focus].aria.label : null;
    })
    .toBe('Subscribe');
  // Keys typed with the mirror element focused reach gpux: Space toggles the focused box.
  expect(await page.evaluate(() => document.activeElement?.getAttribute('aria-label'))).toBe('Subscribe');
  await page.keyboard.press('Space');
  await expectStatus(page, 'clicks 0 subscribed true volume 5');
});

test('screen-reader: input gpux focus moves DOM focus without sending it back @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  const before = await actionsSent(page);
  // A pointer click on the canvas focuses the check box in gpux.
  await gpux.click('role=checkbox name=Subscribe');
  await expect
    .poll(() => page.evaluate(() => document.activeElement?.getAttribute('aria-label')))
    .toBe('Subscribe');
  expect(await actionsSent(page), 'no Focus request echoes the move').toBe(before);
});

test('screen-reader: input the focused text field is the IME textarea @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  await gpux.click('role=textbox name=Title');
  await expect
    .poll(() =>
      page.evaluate(() => ({
        tag: document.activeElement?.tagName,
        label: document.activeElement?.getAttribute('aria-label'),
      })),
    )
    .toEqual({ tag: 'TEXTAREA', label: 'Title' });
  await page.keyboard.type('Hi');
  await expect(mirror(page).getByRole('textbox', { name: 'Title' })).toHaveText('Hi');
});

test('screen-reader: activation no mirror and no tree before the button @pr', async ({ gpux, page }) => {
  await gpux.open('accessibility', { params: { inspect: '0' } });
  await expect(page.getByRole('button', { name: 'Enable accessibility' })).toHaveCount(1);
  await gpux.frames(4);
  await expect(mirror(page)).toHaveCount(0);
  expect(
    await page.evaluate(() => (globalThis.__gpuxConformance as unknown as A11yHost).a11yTree()),
  ).toBeNull();
});

test('screen-reader: activation the enable button builds the mirror @pr', async ({ gpux, page }) => {
  await gpux.open('accessibility', { params: { inspect: '0' } });
  const enable = page.getByRole('button', { name: 'Enable accessibility' });
  // First in the window's DOM, before the canvas.
  expect(
    await enable.evaluate(
      (button) =>
        !!(button.compareDocumentPosition(document.querySelector('canvas')!) & Node.DOCUMENT_POSITION_FOLLOWING),
    ),
  ).toBe(true);
  await enable.focus();
  await page.keyboard.press('Enter');
  await expect(mirror(page).getByRole('button', { name: 'Activate' })).toHaveCount(1);
  await expect(enable).toHaveCount(0);
});

test('screen-reader: activation always builds the mirror with no button @pr', async ({ gpux, page }) => {
  await gpux.open('accessibility', { params: { inspect: '0', 'screen-reader': 'always' } });
  await expect(mirror(page).getByRole('checkbox', { name: 'Subscribe' })).toHaveCount(1);
  await expect(page.locator('[data-gpux-a11y-enable]')).toHaveCount(0);
});

test('screen-reader: activation the button label is the application\'s @pr', async ({ gpux, page }) => {
  await gpux.open('accessibility', {
    params: { inspect: '0', 'screen-reader-label': 'Barrierefreiheit aktivieren' },
  });
  await expect(page.getByRole('button', { name: 'Barrierefreiheit aktivieren' })).toHaveCount(1);
});

test('screen-reader: tree states, relations and positions reach ARIA @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  const root = mirror(page);
  const details = root.getByRole('button', { name: 'Details' });
  await expect(details).toHaveAttribute('aria-expanded', 'false');
  await expect(root.getByRole('button', { name: 'Delete' })).toHaveAttribute('aria-disabled', 'true');
  const code = root.getByRole('textbox', { name: 'Code' });
  await expect(code).toHaveAttribute('aria-readonly', 'true');
  await expect(code).toHaveAttribute('aria-invalid', 'true');
  const error = (await code.getAttribute('aria-errormessage'))!;
  await expect(page.locator(`[id="${error}"]`)).toHaveText('Six digits');
  const pear = root.getByRole('listitem').filter({ hasText: 'Pear' });
  await expect(pear).toHaveAttribute('aria-selected', 'true');
  await expect(pear).toHaveAttribute('aria-posinset', '2');
  await expect(pear).toHaveAttribute('aria-setsize', '3');
  const scores = root.getByRole('table', { name: 'Scores' });
  await expect(scores).toHaveAttribute('aria-rowcount', '2');
  await expect(scores.getByRole('row').nth(1)).toHaveAttribute('aria-rowindex', '2');
  await expect(scores.getByRole('cell').nth(1)).toHaveAttribute('aria-colindex', '2');
});

test('screen-reader: tree drops removed nodes and keeps a node that loses its role in order @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  const root = mirror(page);
  // A node that leaves the tree leaves the DOM; what it controlled is named again when back.
  await gpux.click('role=button name=Details');
  const panel = root.getByRole('region', { name: 'Details panel' });
  await expect(panel).toHaveCount(1);
  await expect(root.getByRole('button', { name: 'Details' })).toHaveAttribute(
    'aria-controls',
    (await panel.getAttribute('id'))!,
  );
  await gpux.click('role=button name=Details');
  await expect(panel).toHaveCount(0);
  // The links' box becomes a named group, then a box with no node again: the links keep their
  // elements and their order either way.
  const alpha = root.getByRole('link', { name: 'Alpha' });
  await alpha.evaluate((element) => ((element as unknown as { kept: boolean }).kept = true));
  await gpux.click('role=button name=Pin');
  const pinned = root.getByRole('group', { name: 'Pinned' });
  await expect(pinned.getByRole('link')).toHaveText(['Alpha', 'Beta']);
  await gpux.click('role=button name=Pin');
  await expect(pinned).toHaveCount(0);
  const links = root.getByRole('link');
  await expect(links).toHaveText(['Alpha', 'Beta']);
  expect(await alpha.evaluate((element) => (element as unknown as { kept?: boolean }).kept)).toBe(true);
});

test('screen-reader: tree updates touch only the changed nodes @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  await page.evaluate((selector) => {
    const root = document.querySelector(selector)!;
    const touched = new Set<Element>();
    (globalThis as unknown as { __touched: Set<Element> }).__touched = touched;
    new MutationObserver((records) => {
      for (const record of records) {
        const target = record.target instanceof Element ? record.target : record.target.parentElement;
        if (target && target !== root) touched.add(target.closest('[data-gpux-node]') ?? target);
      }
    }).observe(root, { subtree: true, attributes: true, childList: true, characterData: true });
  }, MIRROR);
  await gpux.click('role=checkbox name=Subscribe');
  await expectStatus(page, 'clicks 0 subscribed true volume 5');
  await gpux.frames(4);
  const touched = await page.evaluate(() =>
    [...(globalThis as unknown as { __touched: Set<Element> }).__touched].map(
      (element) => `${element.getAttribute('role') ?? 'text'}:${element.getAttribute('aria-label') ?? element.textContent}`,
    ),
  );
  expect(touched.length).toBeGreaterThan(0);
  for (const element of touched) {
    expect(['checkbox:Subscribe', 'status:', 'text:clicks 0 subscribed true volume 5'].some((allowed) =>
      element.startsWith(allowed),
    ), `${element} changed: ${touched}`).toBe(true);
  }
});

test('screen-reader: tree follows the canvas when the page scrolls @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  await page.evaluate(() => {
    document.body.style.height = '3000px';
    window.scrollTo(0, 120);
  });
  await gpux.frames(2);
  const canvas = (await page.locator('canvas').first().boundingBox())!;
  const button = await gpux.locate('role=button name=Activate');
  const box = (await mirror(page).getByRole('button', { name: 'Activate' }).boundingBox())!;
  near(box, { ...button, x: button.x + canvas.x, y: button.y + canvas.y });
});

test('screen-reader: input disclosure keys expand and collapse @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  const details = mirror(page).getByRole('button', { name: 'Details' });
  await details.focus();
  await page.keyboard.press('Enter');
  await expect(details).toHaveAttribute('aria-expanded', 'true');
  await expect(mirror(page).getByRole('region', { name: 'Details panel' })).toHaveCount(1);
  await details.focus();
  await page.keyboard.press('Enter');
  await expect(details).toHaveAttribute('aria-expanded', 'false');
  await expect(mirror(page)).toHaveAttribute('data-gpux-last-action', /^Collapse /);
});

test('screen-reader: input spin button keys step its value @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  const copies = mirror(page).getByRole('spinbutton', { name: 'Copies' });
  await copies.focus();
  await page.keyboard.press('ArrowUp');
  await expect(copies).toHaveAttribute('aria-valuenow', '2');
  await page.keyboard.press('ArrowDown');
  await expect(copies).toHaveAttribute('aria-valuenow', '1');
});

test('screen-reader: input focusing a node scrolled out of view scrolls it in @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  const shelf = (await mirror(page).getByRole('list', { name: 'Shelf' }).boundingBox())!;
  const item = mirror(page).getByRole('listitem', { name: 'Item 5' });
  expect((await item.boundingBox())!.y).toBeGreaterThan(shelf.y + shelf.height - 1);
  await item.focus();
  await expect(mirror(page)).toHaveAttribute('data-gpux-last-action', /^ScrollIntoView /);
  await expect
    .poll(async () => {
      const box = (await item.boundingBox())!;
      return box.y >= shelf.y - 1 && box.y + box.height <= shelf.y + shelf.height + 1;
    })
    .toBe(true);
});

test('screen-reader: input the IME textarea describes the field, and the window holds focus otherwise @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  // No node has focus: the window holds DOM focus, not an unnamed edit field.
  await expect
    .poll(() => page.evaluate(() => document.activeElement?.hasAttribute('data-gpux-a11y-mirror')))
    .toBe(true);
  await gpux.click('role=textbox name=Title');
  const textarea = page.locator('textarea');
  await expect(textarea).toBeFocused();
  await expect(textarea).toHaveAttribute('aria-label', 'Title');
  await expect(textarea).toHaveAttribute('aria-multiline', 'false');
  await expect(textarea).toHaveAttribute('aria-placeholder', 'Untitled');
  // Focus moves to a check box: the textarea no longer stands for the field.
  await gpux.click('role=checkbox name=Subscribe');
  await expect(textarea).not.toHaveAttribute('aria-label', 'Title');
});

test('screen-reader: input an editor without a node keeps text input on the textarea @pr', async ({ gpux, page }) => {
  await openMirrored(gpux);
  // A custom editor with its own input handler and no role: GPUI focuses it, the tree's focus
  // is the window, and typing, IMEs and paste still go through the textarea.
  // Focus first on a node (the check box), so the tree's focus then moves to the window.
  await gpux.click('role=checkbox name=Subscribe');
  await expect
    .poll(() => page.evaluate(() => document.activeElement?.getAttribute('aria-label')))
    .toBe('Subscribe');
  await page.evaluate(() => {
    const seen: string[] = [];
    (globalThis as unknown as { __focusins: string[] }).__focusins = seen;
    document.addEventListener('focusin', (event) =>
      seen.push((event.target as Element).tagName + ((event.target as Element).hasAttribute('data-gpux-a11y-mirror') ? ':mirror' : '')),
    );
  });
  await gpux.click('text~="pad"');
  await gpux.frames(6);
  await expect(page.locator('textarea')).toBeFocused();
  // DOM focus never left the textarea for the window's root on the way.
  const focusins = await page.evaluate(() => (globalThis as unknown as { __focusins: string[] }).__focusins);
  expect(focusins.filter((target) => target.endsWith(':mirror')), `${focusins}`).toEqual([]);
  const session = await page.context().newCDPSession(page);
  await session.send('Input.insertText', { text: 'ok' });
  await gpux.locate('text="pad ok"');
});
