import { defineConfig, devices } from '@playwright/test';

// `gpux-testkit web e2e` serves the build and passes its address; `gpux-testkit web serve`
// listens on port 8000 for manual runs.
const baseURL = process.env.GPUX_BASE_URL ?? 'http://127.0.0.1:8000/';
// Raw output stays in the repository's ignored artifacts folder.
const artifacts = '../artifacts-temp/testkit/b';
const ci = !!process.env.CI;
// gpui_web drives the experimental HTML-in-canvas API only on pages that opt into HTML islands;
// enabling it lets the island specs exercise that path.
const htmlInCanvas = '--enable-blink-features=CanvasDrawElement';

export default defineConfig({
  testDir: './specs',
  outputDir: `${artifacts}/test-results`,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  fullyParallel: true,
  forbidOnly: ci,
  retries: 0,
  workers: ci ? 2 : undefined,
  // The testkit counts results from the list reporter; CI also keeps blob reports to merge.
  reporter: ci
    ? [['list'], ['blob', { outputDir: `${artifacts}/blob-report` }]]
    : [['list']],
  use: {
    ...devices['Desktop Chrome'],
    baseURL,
    viewport: { width: 1280, height: 720 },
    deviceScaleFactor: 1,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      // The implementer's real GPU through new headless Chromium (or `--headed`).
      name: 'local-gpu',
      use: {
        channel: 'chromium',
        launchOptions: { args: ['--enable-unsafe-webgpu', htmlInCanvas] },
      },
    },
    {
      // GitHub's ubuntu runners: no GPU and no display. Chromium keeps only the last
      // `--enable-features`, so Playwright's own screenshot feature is repeated here.
      name: 'ci-linux-swiftshader',
      use: {
        launchOptions: {
          args: [
            '--enable-unsafe-webgpu',
            '--enable-features=Vulkan,CDPScreenshotNewSurface',
            '--use-angle=swiftshader',
            '--use-vulkan=swiftshader',
            htmlInCanvas,
          ],
        },
      },
    },
    {
      // Windows without a usable GPU: WebGPU on the WARP software adapter.
      name: 'windows-software',
      use: {
        channel: 'chromium',
        launchOptions: { args: ['--use-angle=d3d11-warp', '--enable-unsafe-webgpu', htmlInCanvas] },
      },
    },
  ],
});
