import { expect, test } from '../fixtures/gpux';

// WASM threads: a queue shared by the browser main thread and real web workers moves every item
// under contention, and a main-thread atomic wait traps. Migrated from gpui_web's hello_web
// `test_queue.mjs`.

test('the main thread and workers share a queue under contention @pr', async ({ gpux, page }) => {
  test.setTimeout(180_000);
  await gpux.open('paint-borders');
  const result = await page.evaluate(async () => {
    if (!crossOriginIsolated) throw new Error('shared memory needs the isolation headers');
    const fixture = (await import('/web_host.js')) as unknown as {
      forbiddenMainThreadWait(): void;
      QueueStress: new (workers: number, items: number) => {
        ready(): boolean;
        send_batch(count: number): void;
        drain(): number;
        stop(): void;
        stopped(): boolean;
        free(): void;
      };
    };
    let trapped = false;
    try {
      fixture.forbiddenMainThreadWait();
    } catch (error) {
      trapped = error instanceof WebAssembly.RuntimeError;
    }
    const workers = 8;
    const rounds = 128;
    const batch = 4096;
    const stress = new fixture.QueueStress(workers, rounds * batch);
    const yieldToBrowser = () => new Promise((resolve) => setTimeout(resolve, 2));
    while (!stress.ready()) await yieldToBrowser();
    for (let round = 0; round < rounds; round++) {
      stress.send_batch(batch);
      while (stress.drain() !== (round + 1) * batch) await yieldToBrowser();
      // Let the queue empty so workers repeatedly park and get notified.
      await yieldToBrowser();
    }
    stress.stop();
    while (!stress.stopped()) await yieldToBrowser();
    stress.free();
    return { trapped, items: rounds * batch };
  });
  expect(result.trapped, 'a main-thread atomic wait must trap').toBe(true);
  expect(result.items).toBe(128 * 4096);
});
