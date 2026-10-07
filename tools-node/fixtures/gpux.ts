import { test as base, expect, type Page } from '@playwright/test';
import { decodePng, distinctColors, pixel, type Image } from './png';

export { expect };

/** The browser backends `web_host` reports. */
export type Backend = 'webgpu' | 'webgl';

/** CSS-pixel bounds of a located element. */
export interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type Rgba = [number, number, number, number];

/** A scene's pixel expectation for its first frame, as `web_host` publishes it. */
export interface Probe {
  x: number;
  y: number;
  color: Rgba;
  tolerance: number;
}

/**
 * Console errors any spec tolerates. Each entry needs a reason; a spec that expects a specific
 * error allows it locally with `gpux.allowConsole`.
 */
export const CONSOLE_ALLOW_LIST: RegExp[] = [];

export interface OpenOptions {
  /** Forces a backend (`?backend=`). */
  backend?: Backend;
  /** The backend the scene must report: the forced one, WebGPU otherwise. */
  expectBackend?: Backend;
  /** Extra URL parameters, such as `gpux-html-island`. */
  params?: Record<string, string>;
}

/**
 * Dawn reports validation problems as console warnings; any of these fails the spec, as does an
 * uncaptured WebGPU error (see [`WEBGPU_ERROR_HOOK`]).
 */
const WEBGPU_VALIDATION = /While (calling|validating|encoding|finishing)|\[Invalid \w+|GPUValidationError|WebGPU uncaptured error/;

/** Records every uncaptured error of every WebGPU device the page creates. */
const WEBGPU_ERROR_HOOK = () => {
  const errors: string[] = [];
  (globalThis as unknown as { __gpuxWebGpuErrors: string[] }).__gpuxWebGpuErrors = errors;
  const adapter = (globalThis as unknown as { GPUAdapter?: { prototype: { requestDevice: Function } } })
    .GPUAdapter;
  if (!adapter) return;
  const requestDevice = adapter.prototype.requestDevice;
  adapter.prototype.requestDevice = async function (this: unknown, ...args: unknown[]) {
    const device = await requestDevice.apply(this, args);
    device.addEventListener('uncapturederror', (event: { error: { message: string } }) => {
      errors.push(event.error.message);
      console.error(`WebGPU uncaptured error: ${event.error.message}`);
    });
    return device;
  };
};

declare global {
  // eslint-disable-next-line no-var
  var __gpuxConformance:
    | {
        ready: boolean;
        done: boolean;
        backend?: string;
        scene: string | null;
        results: string[];
        probes: Probe[];
        lastError?: string;
        lastLocateError?: string;
        locate(query: string): Bounds | null;
      }
    | undefined;
  // eslint-disable-next-line no-var
  var __gpuxThreadDiagnostics:
    | { crossOriginIsolated: boolean; sharedArrayBuffer: boolean; atomicsWaitAsync: boolean }
    | undefined;
}

/** Drives `web_host`: opens scenes, locates elements, sends trusted input and probes pixels. */
export class Gpux {
  /** Console errors, uncaught exceptions and crashes; any fails the spec. */
  readonly problems: string[] = [];
  private readonly allowed: RegExp[] = [...CONSOLE_ALLOW_LIST];

  constructor(readonly page: Page) {
    page.on('console', (message) => {
      const text = message.text();
      const failing =
        message.type() === 'error' || (message.type() === 'warning' && WEBGPU_VALIDATION.test(text));
      if (failing && !this.allowed.some((pattern) => pattern.test(text))) {
        this.problems.push(`console ${message.type()}: ${text}`);
      }
    });
    page.on('pageerror', (error) => this.problems.push(`uncaught exception: ${error.message}`));
    page.on('crash', () => this.problems.push('the page crashed'));
  }

  /** Installs the page hooks every spec runs with. */
  async install(): Promise<void> {
    await this.page.addInitScript(WEBGPU_ERROR_HOOK);
  }

  /** Calls an export of the loaded `web_host` module (its fixtures). */
  async callExport<T>(name: string, ...args: unknown[]): Promise<T> {
    return this.page.evaluate(
      async ({ name, args }) => {
        const module = (await import('/web_host.js')) as Record<string, (...args: unknown[]) => T>;
        return module[name](...args);
      },
      { name, args },
    );
  }

  /** Tolerates console errors matching `pattern` for the rest of the spec. */
  allowConsole(pattern: RegExp): void {
    this.allowed.push(pattern);
  }

  /** Shows a scene and waits until it has drawn, then checks isolation and the backend. */
  async open(scene: string, options: OpenOptions = {}): Promise<void> {
    const params = new URLSearchParams({ scene, ...options.params });
    if (options.backend) params.set('backend', options.backend);
    await this.page.goto(`/?${params}`);
    await this.page.waitForFunction(
      () => globalThis.__gpuxConformance?.ready === true || !!globalThis.__gpuxConformance?.lastError,
      undefined,
      { timeout: 30_000 },
    );
    const error = await this.page.evaluate(() => globalThis.__gpuxConformance?.lastError);
    if (error) throw new Error(`${scene} did not start: ${error}`);
    await this.expectIsolated();
    await this.expectBackend(options.expectBackend ?? options.backend ?? 'webgpu');
  }

  /** Runs scenes' scripts in-app (`?run=<scene>` or `?run=all`) and returns their result lines. */
  async runScripts(scene: string | 'all', options: OpenOptions = {}): Promise<string[]> {
    const params = new URLSearchParams(
      scene === 'all' ? { run: 'all', ...options.params } : { scene, run: '1', ...options.params },
    );
    if (options.backend) params.set('backend', options.backend);
    await this.page.goto(`/?${params}`);
    await this.page.waitForFunction(
      () => globalThis.__gpuxConformance?.done === true || !!globalThis.__gpuxConformance?.lastError,
      undefined,
      { timeout: 120_000 },
    );
    const state = await this.page.evaluate(() => ({
      error: globalThis.__gpuxConformance?.lastError,
      results: globalThis.__gpuxConformance?.results ?? [],
    }));
    if (state.error) throw new Error(`${scene}: ${state.error}`);
    return state.results;
  }

  /** The page is cross-origin isolated, so WASM threads share memory. */
  async expectIsolated(): Promise<void> {
    const isolation = await this.page.evaluate(() => ({
      isolated: globalThis.crossOriginIsolated,
      diagnostics: globalThis.__gpuxThreadDiagnostics,
    }));
    expect(isolation.isolated, 'the page must be cross-origin isolated').toBe(true);
    expect(isolation.diagnostics?.sharedArrayBuffer).toBe(true);
  }

  /** The backend `web_host` reports. */
  async backend(): Promise<string | undefined> {
    return this.page.evaluate(() => globalThis.__gpuxConformance?.backend);
  }

  async expectBackend(backend: Backend): Promise<void> {
    expect(await this.backend(), `the scene must render through ${backend}`).toBe(backend);
  }

  /** Waits for `count` browser animation frames, so input and state changes are presented. */
  async frames(count = 2): Promise<void> {
    await this.page.evaluate(
      (count) =>
        new Promise<void>((resolve) => {
          let left = count;
          const tick = () => (--left <= 0 ? resolve() : requestAnimationFrame(tick));
          requestAnimationFrame(tick);
        }),
      count,
    );
  }

  /** The bounds of the element `query` matches in the latest frame, or `null`. */
  async find(query: string): Promise<Bounds | null> {
    return this.page.evaluate((query) => globalThis.__gpuxConformance!.locate(query), query);
  }

  /** The bounds of the element `query` matches, waiting for it to appear. */
  async locate(query: string): Promise<Bounds> {
    let bounds: Bounds | null = null;
    await expect
      .poll(
        async () => {
          bounds = await this.find(query);
          return bounds !== null;
        },
        { message: `an element matching \`${query}\``, timeout: 10_000 },
      )
      .toBe(true);
    return bounds!;
  }

  /** Waits until no element matches `query`. */
  async expectMissing(query: string): Promise<void> {
    await expect
      .poll(async () => (await this.find(query)) === null, {
        message: `no element matching \`${query}\``,
        timeout: 10_000,
      })
      .toBe(true);
  }

  /** The center of the element `query` matches. */
  async center(query: string): Promise<{ x: number; y: number }> {
    const bounds = await this.locate(query);
    return { x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 };
  }

  /** Clicks the element's center with trusted input, then waits for it to be presented. */
  async click(query: string, options: { clickCount?: number } = {}): Promise<void> {
    const { x, y } = await this.center(query);
    await this.page.mouse.click(x, y, options);
    await this.frames();
  }

  /** A screenshot of a CSS-pixel region, decoded. */
  async capture(region?: Bounds): Promise<Image> {
    const png = await this.page.screenshot({
      clip: region,
      scale: 'css',
      animations: 'disabled',
    });
    return decodePng(png);
  }

  /** The color at a CSS-pixel position. */
  async probe(x: number, y: number): Promise<Rgba> {
    await this.frames();
    return pixel(await this.capture({ x: Math.floor(x), y: Math.floor(y), width: 1, height: 1 }), 0, 0);
  }

  /** Asserts the color at a position, within `tolerance` per channel. */
  async expectColor(x: number, y: number, color: Rgba, tolerance = 3): Promise<void> {
    await expect
      .poll(
        async () => {
          const actual = await this.probe(x, y);
          return actual.every((channel, index) => Math.abs(channel - color[index]) <= tolerance)
            ? 'match'
            : `rgba(${actual.join(', ')})`;
        },
        { message: `the pixel at (${x}, ${y}) should be rgba(${color.join(', ')})`, timeout: 10_000 },
      )
      .toBe('match');
  }

  /** Asserts the scene's own first-frame probes. */
  async expectFirstFrameProbes(): Promise<number> {
    const probes = await this.page.evaluate(() => globalThis.__gpuxConformance?.probes ?? []);
    for (const probe of probes) {
      await this.expectColor(probe.x, probe.y, probe.color, probe.tolerance);
    }
    return probes.length;
  }

  /** How many distinct colors a region shows (a flat region shows one). */
  async distinctColors(region?: Bounds): Promise<number> {
    await this.frames();
    return distinctColors(await this.capture(region));
  }
}

/** `test` with a `gpux` fixture that fails the spec on console errors, exceptions or crashes. */
export const test = base.extend<{ gpux: Gpux }>({
  gpux: async ({ page }, use) => {
    const gpux = new Gpux(page);
    await gpux.install();
    await use(gpux);
    expect(gpux.problems, 'the page reported problems').toEqual([]);
  },
});
