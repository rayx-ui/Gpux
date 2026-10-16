import { expect, test } from '../fixtures/gpux';

// The accessibility tree in the browser: a window builds its AccessKit tree whenever inspection
// asks for it, with no screen reader and before any screen-reader adapter exists. The web host
// inspects a shown scene unless the page asks it not to (`?inspect=0`); these specs start
// inspection themselves through `startA11yInspection()` and read the tree with `a11yTree()` (the
// tree's debug JSON: nodes keyed by short ids, each with its `aria` section).

interface A11yHost {
  startA11yInspection(): boolean;
  a11yTree(): string | null;
}

interface A11yNode {
  aria: { role: string; label?: string };
}

test('accessibility: no tree is built before inspection starts @pr', async ({ gpux, page }) => {
  await gpux.open('semantics-accessibility', { params: { inspect: '0' } });
  // Frames go by; nothing asked for the tree, so there is none.
  await page.waitForTimeout(250);
  const tree = await page.evaluate(() =>
    (globalThis.__gpuxConformance as unknown as A11yHost).a11yTree(),
  );
  expect(tree).toBeNull();
});

test("accessibility: inspection builds a tree holding the scene's buttons @pr", async ({ gpux, page }) => {
  await gpux.open('semantics-accessibility', { params: { inspect: '0' } });
  const started = await page.evaluate(() =>
    (globalThis.__gpuxConformance as unknown as A11yHost).startA11yInspection(),
  );
  expect(started).toBe(true);
  const read = () =>
    page.evaluate(() => (globalThis.__gpuxConformance as unknown as A11yHost).a11yTree());
  await expect.poll(read).not.toBeNull();
  const tree = JSON.parse((await read()) as string) as { nodes: Record<string, A11yNode> };
  const nodes = Object.values(tree.nodes).map((node) => node.aria);
  expect(nodes.length).toBeGreaterThan(1);
  expect(nodes).toContainEqual(expect.objectContaining({ role: 'Button', label: 'Submit' }));
  expect(nodes).toContainEqual(expect.objectContaining({ role: 'Button', label: 'Cancel' }));
});
