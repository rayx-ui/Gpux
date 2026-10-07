import type { CDPSession } from '@playwright/test';
import { expect, test } from '../fixtures/gpux';

// Trusted touch input through CDP: a tap clicks, a cancelled touch does not, a pan scrolls, and a
// fast pan flings on after the finger lifts.

test.use({ hasTouch: true });

async function touch(
  session: CDPSession,
  type: string,
  x?: number,
  y?: number,
  timestamp?: number,
) {
  await session.send('Input.dispatchTouchEvent', {
    type,
    touchPoints: x === undefined ? [] : [{ x, y: y!, id: 1, radiusX: 1, radiusY: 1, force: 1 }],
    ...(timestamp === undefined ? {} : { timestamp }),
  });
}

/**
 * Pans from `(x, y)` by `dy` over `steps` 16 ms frames. Each event carries its own timestamp,
 * which gpui_web passes to gpui's gesture velocity, so the gesture's speed does not depend on how
 * fast the host delivers the CDP calls.
 */
async function pan(session: CDPSession, x: number, y: number, dy: number, steps: number) {
  const start = Date.now() / 1000;
  const at = (step: number) => start + step * 0.016;
  await touch(session, 'touchStart', x, y, at(0));
  for (let step = 1; step <= steps; step++) {
    await touch(session, 'touchMove', x, y + (dy * step) / steps, at(step));
  }
  await touch(session, 'touchEnd', undefined, undefined, at(steps));
}

test('a tap clicks the element under it @pr', async ({ gpux, page }) => {
  await gpux.open('overlay-anchored');
  const session = await page.context().newCDPSession(page);
  const { x, y } = await gpux.center('id=toggle');
  await touch(session, 'touchStart', x, y);
  await page.waitForTimeout(16);
  await touch(session, 'touchEnd');
  await gpux.frames();
  expect(await gpux.locate('id=popover')).toEqual({ x: 20, y: 48, width: 100, height: 50 });
});

/** The scroll-wheel scene's offset, read from its status line. */
function offset(page: import('@playwright/test').Page): Promise<number | null> {
  return page.evaluate(() => {
    for (let offset = 0; offset <= 2000; offset++) {
      if (globalThis.__gpuxConformance!.locate(`role=status value="offset ${offset}"`)) return offset;
    }
    return null;
  });
}

test('a cancelled touch does not click @pr', async ({ gpux, page }) => {
  await gpux.open('overlay-anchored');
  const session = await page.context().newCDPSession(page);
  const { x, y } = await gpux.center('id=toggle');
  await touch(session, 'touchStart', x, y);
  await page.waitForTimeout(16);
  await touch(session, 'touchCancel');
  await gpux.frames(3);
  await gpux.expectMissing('id=popover');
});

test('a fast pan flings on after the finger lifts @pr', async ({ gpux, page }) => {
  await gpux.open('scroll-wheel');
  const session = await page.context().newCDPSession(page);
  const { x, y } = await gpux.center('id=scroller');
  // 60 pixels in 64 milliseconds: well above the fling threshold.
  await pan(session, x, y, -60, 4);
  await gpux.frames();
  const lifted = (await offset(page))!;
  await page.waitForTimeout(500);
  await gpux.frames();
  expect(await offset(page)).toBeGreaterThan(lifted);
});

test('a pan scrolls the element under it @pr', async ({ gpux, page }) => {
  await gpux.open('scroll-wheel');
  const session = await page.context().newCDPSession(page);
  const { x, y } = await gpux.center('id=scroller');
  await pan(session, x, y, -60, 6);
  await page.waitForTimeout(500);
  await gpux.frames();
  // The pan moved the content up by at least its distance (a fling may carry it further).
  expect(await offset(page)).toBeGreaterThanOrEqual(50);
});
