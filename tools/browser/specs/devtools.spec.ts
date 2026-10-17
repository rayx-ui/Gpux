import type { Page, WebSocketRoute } from '@playwright/test';
import { expect, test } from '../fixtures/gpux';

// gpux-devtools in the browser: the page's app connects out over a WebSocket to the sidecar the
// `gpux_devtools` parameter names. Here the spec is that sidecar (Playwright answers the page's
// socket itself): it reads the app's `hello`, welcomes it, and asks for a snapshot, a query and a
// synthesized click on the `web-devtools` scene's button, whose effect the app's tree then shows.

const SIDECAR = 'ws://127.0.0.1:9230';

interface Message {
  jsonrpc: '2.0';
  id?: number;
  method?: string;
  params?: Record<string, unknown>;
  result?: Record<string, unknown>;
  error?: { code: number; message: string };
}

/** The spec's side of the connection. */
class Sidecar {
  private socket?: WebSocketRoute;
  private readonly received: Message[] = [];
  private nextId = 100;

  async serve(page: Page): Promise<void> {
    await page.routeWebSocket(SIDECAR, (socket) => {
      this.socket = socket;
      socket.onMessage((data) => this.received.push(JSON.parse(String(data)) as Message));
    });
  }

  /** The first message matching `match`, waiting for it. */
  async next(match: (message: Message) => boolean, what: string): Promise<Message> {
    let found: Message | undefined;
    await expect
      .poll(() => (found = this.received.find(match)) !== undefined, { message: what, timeout: 30_000 })
      .toBe(true);
    this.received.splice(this.received.indexOf(found!), 1);
    return found!;
  }

  send(message: Message): void {
    this.socket!.send(JSON.stringify(message));
  }

  /** Reads the app's `hello` and welcomes it. */
  async handshake(): Promise<Message> {
    const hello = await this.next((message) => message.method === 'hello', 'the app says hello');
    this.send({ jsonrpc: '2.0', id: hello.id, result: { protocolVersion: 1 } });
    return hello;
  }

  /** Asks the app `method` and returns its result. */
  async request(method: string, params: Record<string, unknown>): Promise<Record<string, unknown>> {
    const id = this.nextId++;
    this.send({ jsonrpc: '2.0', id, method, params });
    const answer = await this.next((message) => message.id === id && !message.method, `the answer to ${method}`);
    expect(answer.error, `${method}: ${answer.error?.message}`).toBeUndefined();
    return answer.result!;
  }
}

async function connected(gpux: import('../fixtures/gpux').Gpux, page: Page) {
  const sidecar = new Sidecar();
  await sidecar.serve(page);
  await gpux.open('web-devtools', { params: { gpux_devtools: SIDECAR } });
  const hello = await sidecar.handshake();
  return { sidecar, hello };
}

test('devtools: the page says hello and a snapshot shows its window @pr', async ({ gpux, page }) => {
  const { sidecar, hello } = await connected(gpux, page);
  const params = hello.params as { appId: string; windows: { id: number }[]; protocolVersion: number };
  expect(params.appId).toBe('dev.gpux.conformance');
  expect(params.protocolVersion).toBe(1);
  expect(params.windows.length).toBe(1);
  const window = params.windows[0].id;
  // The first frame may still be on its way.
  let snapshot = '';
  await expect
    .poll(async () => {
      snapshot = String((await sidecar.request('snapshot', { window })).snapshot);
      return snapshot;
    })
    .toContain('button "Tap me"');
  expect(snapshot).toMatch(/^- window/);
  expect(snapshot).toContain('taps 0');
});

test('devtools: a click through the sidecar reaches the button @pr', async ({ gpux, page }) => {
  const { sidecar, hello } = await connected(gpux, page);
  const window = (hello.params as { windows: { id: number }[] }).windows[0].id;
  let reference: string | undefined;
  await expect
    .poll(async () => {
      const found = await sidecar.request('query', { window, query: 'role=button name="Tap me"' });
      reference = (found.nodes as { ref: string }[])[0]?.ref;
      return reference;
    })
    .toBeDefined();
  await sidecar.request('input', { window, kind: 'click', ref: reference });
  const waited = await sidecar.request('wait', { window, query: 'text="taps 1"', timeoutMs: 10_000 });
  expect(waited.matched).toBe(true);
});
