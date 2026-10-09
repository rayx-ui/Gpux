import type { Page } from '@playwright/test';
import { type Gpux, expect, test } from '../fixtures/gpux';

// Wheel input in every DOM delta mode reaches gpui as the matching scroll distance.

/** `WheelEvent.DOM_DELTA_*`, which Node does not define. */
const WheelEvent_DOM_DELTA = { PIXEL: 0, LINE: 1, PAGE: 2 } as const;

/** The scroll-wheel scene's offset, read from its status line. */
async function offset(gpux: Gpux): Promise<number> {
  const value = await gpux.page.evaluate(() => {
    for (let offset = 0; offset <= 2000; offset++) {
      if (globalThis.__gpuxConformance!.locate(`role=status value="offset ${offset}"`)) return offset;
    }
    return null;
  });
  if (value === null) throw new Error('the scroll-wheel status shows no offset');
  return value;
}

/** Dispatches a wheel event over the scroller in a given DOM delta mode. */
async function wheel(page: Page, at: { x: number; y: number }, deltaY: number, deltaMode: number) {
  await page.evaluate(
    ({ at, deltaY, deltaMode }) => {
      document.querySelector('canvas')!.dispatchEvent(
        new WheelEvent('wheel', {
          clientX: at.x,
          clientY: at.y,
          deltaY,
          deltaMode,
          bubbles: true,
          cancelable: true,
        }),
      );
    },
    { at, deltaY, deltaMode },
  );
}

async function scrolledBy(gpux: Gpux, deltaY: number, deltaMode: number): Promise<number> {
  await gpux.open('scroll-wheel');
  const at = await gpux.center('id=scroller');
  await gpux.page.mouse.move(at.x, at.y);
  await wheel(gpux.page, at, deltaY, deltaMode);
  await gpux.frames(3);
  return offset(gpux);
}

test('pixel deltas scroll by pixels @pr', async ({ gpux }) => {
  expect(await scrolledBy(gpux, 30, WheelEvent_DOM_DELTA.PIXEL)).toBe(30);
});

test('line deltas scroll by whole lines @pr', async ({ gpux }) => {
  const one = await scrolledBy(gpux, 1, WheelEvent_DOM_DELTA.LINE);
  expect(one).toBeGreaterThan(0);
  expect(await scrolledBy(gpux, 3, WheelEvent_DOM_DELTA.LINE)).toBe(3 * one);
});

test('page deltas scroll by the visible page @pr', async ({ gpux, page }) => {
  const scrolled = await scrolledBy(gpux, 0.05, WheelEvent_DOM_DELTA.PAGE);
  const height = await page.evaluate(() => document.querySelector('canvas')!.clientHeight);
  expect(scrolled).toBe(Math.round(0.05 * height));
});

test('trusted wheel input scrolls the hovered element @pr', async ({ gpux, page }) => {
  await gpux.open('scroll-wheel');
  const at = await gpux.center('id=scroller');
  await page.mouse.move(at.x, at.y);
  await page.mouse.wheel(0, 40);
  await gpux.frames(3);
  expect(await offset(gpux)).toBe(40);
});
