# gpux-devtools-mcp

One MCP sidecar for every gpux app. Apps that link [`gpux-devtools`](../gpux_devtools) connect
out to it over a WebSocket on loopback; an agent harness talks to it over stdio and gets MCP tools
that read and drive the apps' AccessKit trees: `gpux_*` tools (list apps, snapshot, query, act,
input, screenshot, wait, stats), the Playwright-compatible `browser_*` tools, and the tools each
app advertises.

```text
gpux-devtools-mcp [--port PORT] [--token TOKEN] [--print-manifest]
```

See [`docs/devtools.md`](https://github.com/rayx-ui/Gpux/blob/main/docs/devtools.md).
