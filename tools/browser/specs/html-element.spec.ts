import { runScript } from '../fixtures/driver';
import { expect, test } from '../fixtures/gpux';

// Trusted DOM overlays (`Window::paint_html_element`): the DOM node follows the element's layout
// bounds, receives its own input, and is pruned when the element is absent.

test('the overlay paints where the element is laid out and follows its presence @pr', async ({ gpux }) => {
  await gpux.open('web-html-element');
  expect((await runScript(gpux)).ran).toBeGreaterThan(5);
});

test('the DOM node sits at the element bounds and owns its input @pr', async ({ gpux, page }) => {
  await gpux.open('web-html-element');
  const button = page.locator('#gpux-dom-button');
  await expect(button).toHaveCount(1);
  expect(await button.boundingBox()).toEqual({ x: 20, y: 70, width: 160, height: 50 });

  // A trusted click on the overlay reaches the DOM node, not the scene underneath.
  await button.click();
  await expect(button).toHaveAttribute('data-clicks', '1');
  await gpux.locate('role=status value="dom shown true"');

  // Hiding the element prunes its DOM node; showing it creates a fresh one.
  await gpux.click('id=toggle');
  await gpux.locate('role=status value="dom shown false"');
  await expect(button).toHaveCount(0);
  await gpux.click('id=toggle');
  await expect(button).toHaveCount(1);
  await expect(button).not.toHaveAttribute('data-clicks', '1');
});
