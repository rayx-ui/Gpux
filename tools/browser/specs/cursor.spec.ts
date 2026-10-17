import { expect, test } from '../fixtures/gpux';

// Cursor styles: hovering an element with a cursor style shows the matching CSS cursor.

test('hovered elements show their CSS cursor @pr', async ({ gpux, page }) => {
  await gpux.open('web-cursors');
  const cursor = () => page.evaluate(() => document.body.style.cursor);
  for (const [id, css] of [
    ['pointer', 'pointer'],
    ['text', 'text'],
    ['grab', 'grab'],
  ]) {
    const { x, y } = await gpux.center(`id=${id}`);
    await page.mouse.move(x, y);
    await expect.poll(cursor, { message: `the cursor over ${id}` }).toBe(css);
  }
  // Off every styled element the cursor returns to the default arrow.
  await page.mouse.move(5, 90);
  await expect.poll(cursor).toBe('default');
});
