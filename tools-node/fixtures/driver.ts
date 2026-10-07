import type { CDPSession } from '@playwright/test';
import type { Gpux } from './gpux';

/** A script step as `web_host` publishes it in `__gpuxConformance.steps`. */
export interface StepDescriptor {
  index: number;
  describe: string;
  kind:
    | 'click'
    | 'clickAt'
    | 'hover'
    | 'moveTo'
    | 'press'
    | 'release'
    | 'drag'
    | 'wheel'
    | 'touchPan'
    | 'touchTap'
    | 'type'
    | 'keys'
    | 'keyUp'
    | 'imeCompose'
    | 'imeCommit'
    | 'imeCancel'
    | 'focus'
    | 'advance'
    | 'expect'
    | 'pixels'
    | 'skip';
  query?: string;
  from?: string;
  to?: string;
  x?: number;
  y?: number;
  count?: number;
  dx?: number;
  dy?: number;
  steps?: number;
  text?: string;
  keys?: string[];
  caret?: number;
  ms?: number;
  reason?: string;
}

export interface ScriptResult {
  ran: number;
  skipped: string[];
}

type Bridge = {
  steps: StepDescriptor[];
  check(index: number): string | null;
  pixels(index: number, png: Uint8Array): string | null;
  focusPoint(query: string): { x: number; y: number } | null;
};

/** One 16 ms frame between touch points, as the in-app hosts pace them. */
const TOUCH_FRAME_MS = 16;
/** How long an expectation or pixel step may take to hold after its input. */
const SETTLE_MS = 2_000;
/** `GPUX_TRACE=1` logs every step the driver performs. */
const TRACE = process.env.GPUX_TRACE === '1';

/**
 * Runs a shown scene's script: input steps through trusted browser input, expectations and pixel
 * steps through the app. Throws on the first failing step, like the in-app hosts.
 */
export async function runScript(gpux: Gpux): Promise<ScriptResult> {
  const page = gpux.page;
  const steps = await page.evaluate(
    () => (globalThis.__gpuxConformance as unknown as Bridge).steps,
  );
  const result: ScriptResult = { ran: 0, skipped: [] };
  let touch: CDPSession | undefined;
  const touchSession = async () => (touch ??= await page.context().newCDPSession(page));
  let pointer = { x: 0, y: 0 };
  const move = async (x: number, y: number) => {
    await page.mouse.move(x, y);
    pointer = { x, y };
  };
  const click = async (x: number, y: number, count: number) => {
    await move(x, y);
    for (let clickCount = 1; clickCount <= count; clickCount++) {
      await page.mouse.down({ clickCount });
      // Elements that record the press while painting (interactive text) need the frame.
      await gpux.frames(1);
      await page.mouse.up({ clickCount });
    }
  };
  const touchEvent = async (type: string, x?: number, y?: number) => {
    const session = await touchSession();
    await session.send('Input.dispatchTouchEvent', {
      type,
      touchPoints: x === undefined ? [] : [{ x, y: y!, id: 1, radiusX: 1, radiusY: 1, force: 1 }],
    });
  };
  const focus = async (query: string) => {
    const at = await page.evaluate(
      (query) => (globalThis.__gpuxConformance as unknown as Bridge).focusPoint(query),
      query,
    );
    if (TRACE) console.log(`[gpux] focus ${query}: ${at ? `click (${at.x}, ${at.y})` : 'already inside'}`);
    if (at) {
      await click(at.x, at.y, 1);
      // The focused element registers its text input handler while it paints.
      await gpux.frames(2);
    }
  };
  const settleUntil = async (attempt: () => Promise<string | null>): Promise<string | null> => {
    const deadline = Date.now() + SETTLE_MS;
    for (;;) {
      await gpux.frames(2);
      const problem = await attempt();
      if (problem === null || Date.now() > deadline) return problem;
    }
  };

  for (const step of steps) {
    const fail = (problem: string) => {
      throw new Error(`step ${step.index + 1} ${step.describe}: ${problem}`);
    };
    if (TRACE) console.log(`[gpux] step ${step.index + 1} ${step.kind}: ${step.describe}`);
    switch (step.kind) {
      case 'skip':
        result.skipped.push(step.reason ?? 'skipped');
        continue;
      case 'click': {
        const at = await gpux.center(step.query!);
        await click(at.x, at.y, step.count ?? 1);
        break;
      }
      case 'clickAt':
        await click(step.x!, step.y!, step.count ?? 1);
        break;
      case 'hover': {
        const at = await gpux.center(step.query!);
        await move(at.x, at.y);
        break;
      }
      case 'moveTo':
        await move(step.x!, step.y!);
        break;
      case 'press': {
        const at = await gpux.center(step.query!);
        await move(at.x, at.y);
        await page.mouse.down();
        break;
      }
      case 'release': {
        const at = await gpux.center(step.query!);
        if (at.x !== pointer.x || at.y !== pointer.y) await move(at.x, at.y);
        await page.mouse.up();
        break;
      }
      case 'drag': {
        const start = await gpux.center(step.from!);
        const end = await gpux.center(step.to!);
        await move(start.x, start.y);
        await page.mouse.down();
        const moves = 4;
        for (let index = 1; index <= moves; index++) {
          const t = index / moves;
          await move(start.x + (end.x - start.x) * t, start.y + (end.y - start.y) * t);
          await gpux.frames(2);
        }
        await page.mouse.up();
        break;
      }
      case 'wheel': {
        const at = await gpux.center(step.query!);
        await move(at.x, at.y);
        // Scene deltas are gpui's (negative scrolls content down); DOM deltas are the opposite.
        // Under device-scale emulation Chromium divides CDP wheel deltas by the scale, so the
        // DOM would report half the distance at DPR 2.
        const scale = await page.evaluate(() => devicePixelRatio);
        await page.mouse.wheel(-step.dx! * scale, -step.dy! * scale);
        break;
      }
      case 'touchPan': {
        const start = await gpux.center(step.query!);
        await touchEvent('touchStart', start.x, start.y);
        const steps = step.steps ?? 1;
        for (let index = 1; index <= steps; index++) {
          await page.waitForTimeout(TOUCH_FRAME_MS);
          const t = index / steps;
          await touchEvent('touchMove', start.x + step.dx! * t, start.y + step.dy! * t);
        }
        await page.waitForTimeout(TOUCH_FRAME_MS);
        await touchEvent('touchEnd');
        break;
      }
      case 'touchTap': {
        const at = await gpux.center(step.query!);
        await touchEvent('touchStart', at.x, at.y);
        await page.waitForTimeout(TOUCH_FRAME_MS);
        await touchEvent('touchEnd');
        break;
      }
      case 'type':
        await focus(step.query!);
        await page.keyboard.insertText(step.text!);
        break;
      case 'keys':
        for (const key of step.keys!) {
          await page.keyboard.press(key);
          await gpux.frames(2);
        }
        break;
      case 'keyUp':
        for (const key of step.keys!) {
          await page.keyboard.up(key.split('+').at(-1)!);
        }
        break;
      case 'imeCompose': {
        const session = await touchSession();
        await session.send('Input.imeSetComposition', {
          text: step.text!,
          selectionStart: step.caret!,
          selectionEnd: step.caret!,
        });
        break;
      }
      case 'imeCommit': {
        const session = await touchSession();
        await session.send('Input.insertText', { text: step.text! });
        break;
      }
      case 'imeCancel': {
        const session = await touchSession();
        await session.send('Input.imeSetComposition', {
          text: '',
          selectionStart: 0,
          selectionEnd: 0,
        });
        break;
      }
      case 'focus':
        await focus(step.query!);
        break;
      case 'advance':
        await page.waitForTimeout(step.ms!);
        await gpux.frames(1);
        break;
      case 'expect': {
        const problem = await settleUntil(() =>
          page.evaluate(
            (index) => (globalThis.__gpuxConformance as unknown as Bridge).check(index),
            step.index,
          ),
        );
        if (problem) fail(problem);
        break;
      }
      case 'pixels': {
        const problem = await settleUntil(async () => {
          const png = await page.screenshot({ scale: 'device', animations: 'disabled' });
          return page.evaluate(
            ({ index, png }) =>
              (globalThis.__gpuxConformance as unknown as Bridge).pixels(
                index,
                Uint8Array.from(atob(png), (char) => char.charCodeAt(0)),
              ),
            { index: step.index, png: png.toString('base64') },
          );
        });
        if (problem) fail(problem);
        break;
      }
    }
    result.ran += 1;
  }
  await touch?.detach();
  return result;
}
