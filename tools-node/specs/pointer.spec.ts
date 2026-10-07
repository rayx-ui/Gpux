import { expect, test } from '../fixtures/gpux';

// Trusted pointer input beyond what the scene scripts cover: buttons other than the primary one
// do not click and the browser's own context menu stays closed, the pointer entering and leaving
// an element changes its hover state, and a press held while the pointer moves keeps reaching
// the scene.

test('a right click opens no browser context menu @pr', async ({ gpux, page }) => {
  await gpux.open('overlay-anchored');
  await page.evaluate(() => {
    (window as unknown as { __menus: boolean[] }).__menus = [];
    window.addEventListener('contextmenu', (event) =>
      (window as unknown as { __menus: boolean[] }).__menus.push(event.defaultPrevented),
    );
  });
  const { x, y } = await gpux.center('id=toggle');
  await page.mouse.click(x, y, { button: 'right' });
  expect(await page.evaluate(() => (window as unknown as { __menus: boolean[] }).__menus)).toEqual([true]);
  // A secondary click is not a primary click: the popover stays closed.
  await gpux.frames();
  await gpux.expectMissing('id=popover');
});

test('a held press keeps reaching the scene while the pointer moves @pr', async ({ gpux, page }) => {
  await gpux.open('hover-active');
  const { x, y } = await gpux.center('id=target');
  await page.mouse.move(x, y);
  await page.mouse.down();
  // Leave the target and come back with the button held, then release on it.
  await page.mouse.move(x + 200, y + 200, { steps: 4 });
  await page.mouse.move(x, y, { steps: 4 });
  await page.mouse.up();
  await gpux.frames();
  await gpux.locate('role=status value~="clicks 1"');
});

test('a press released over DOM content above the canvas still ends in gpui @pr', async ({ gpux, page }) => {
  await gpux.open('hover-active');
  // DOM content above the canvas, away from the target, that records what reaches it.
  await page.evaluate(() => {
    const cover = document.createElement('div');
    Object.assign(cover.style, {
      position: 'fixed',
      left: '400px',
      top: '300px',
      width: '200px',
      height: '150px',
      zIndex: '10',
    });
    const seen: string[] = [];
    (window as unknown as { __coverEvents: string[] }).__coverEvents = seen;
    for (const type of ['pointerup', 'mouseup', 'click']) {
      cover.addEventListener(type, () => seen.push(type));
    }
    document.body.appendChild(cover);
  });
  const { x, y } = await gpux.center('id=target');
  await page.mouse.move(x, y);
  await page.mouse.down();
  await gpux.expectColor(x, y, [0, 0, 255, 255]);
  // Leave the canvas for the cover with the button held, and release there. The canvas holds
  // pointer capture, so the release still reaches gpui and the cover sees none of it.
  await page.mouse.move(500, 375, { steps: 4 });
  await page.mouse.up();
  await gpux.frames();
  expect(
    await page.evaluate(() => (window as unknown as { __coverEvents: string[] }).__coverEvents),
  ).toEqual([]);
  // Back over the target with no button down: hovered, not still pressed, and no click.
  await page.mouse.move(x, y, { steps: 4 });
  await gpux.locate('role=status value="hovered true clicks 0"');
  await gpux.expectColor(x, y, [0, 255, 0, 255]);
});

test('the pointer entering and leaving an element changes its hover state @pr', async ({ gpux, page }) => {
  await gpux.open('hover-active');
  await gpux.locate('role=status value="hovered false clicks 0"');
  const { x, y } = await gpux.center('id=target');
  await page.mouse.move(x, y, { steps: 3 });
  await gpux.locate('role=status value="hovered true clicks 0"');
  await page.mouse.move(x + 300, y + 300, { steps: 3 });
  await gpux.locate('role=status value="hovered false clicks 0"');
});

test('only the primary button clicks @pr', async ({ gpux, page }) => {
  await gpux.open('hover-active');
  const { x, y } = await gpux.center('id=target');
  await page.mouse.click(x, y, { button: 'middle' });
  await page.mouse.click(x, y, { button: 'right' });
  await gpux.frames();
  await gpux.locate('role=status value="hovered true clicks 0"');
  await page.mouse.click(x, y);
  await gpux.locate('role=status value="hovered true clicks 1"');
});
