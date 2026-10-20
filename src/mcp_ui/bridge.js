// Minimal MCP Apps view runtime (SEP-1865): JSON-RPC over postMessage with the host.
// Calls render(result, host) for each tool result; the host sends the tool's CallToolResult.
(function () {
  let nextId = 1;
  const pending = new Map();
  const send = (message) => window.parent.postMessage({ jsonrpc: "2.0", ...message }, "*");
  const request = (method, params) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject });
      send({ id, method, params });
    });
  const notify = (method, params) => send({ method, params });
  let host = { theme: matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light", locale: navigator.language };
  const applyTheme = () => document.documentElement.dataset.theme = host.theme || "light";

  window.cider = {
    openLink: (url) => request("ui/open-link", { url }).catch(() => {}),
    // Asks the host to run one of this server's tools (the host confirms with the user).
    callTool: (name, args = {}) => request("tools/call", { name, arguments: args }),
    host: () => host,
  };

  // The body's own height: the document's scrollHeight never drops below the frame, so a card
  // measured that way could grow but never shrink to fit its content.
  const reportSize = () => {
    const box = document.body.getBoundingClientRect();
    notify("ui/notifications/size-changed", { width: Math.ceil(box.width), height: Math.ceil(box.height) });
  };
  new ResizeObserver(reportSize).observe(document.body);

  window.addEventListener("message", (event) => {
    const message = event.data;
    if (!message || message.jsonrpc !== "2.0") return;
    if (message.id != null && pending.has(message.id)) {
      const { resolve, reject } = pending.get(message.id);
      pending.delete(message.id);
      message.error ? reject(message.error) : resolve(message.result);
      return;
    }
    if (message.method === "ui/notifications/tool-result") {
      window.render?.(message.params ?? {}, host);
      reportSize();
    } else if (message.method === "ui/notifications/host-context-changed") {
      host = { ...host, ...(message.params ?? {}) };
      applyTheme();
    }
  });

  // Links inside a card open through the host (in the user's browser), never in the frame.
  document.addEventListener("click", (event) => {
    const link = event.target.closest?.("a[href]");
    if (!link) return;
    event.preventDefault();
    window.cider.openLink(link.href);
  });

  applyTheme();
  request("ui/initialize", {
    protocolVersion: "2026-01-26",
    appInfo: { name: "cider", version: "1" },
    appCapabilities: { availableDisplayModes: ["inline"] },
  })
    .then((result) => {
      host = { ...host, ...(result?.hostContext ?? {}) };
      applyTheme();
    })
    .catch(() => {})
    .finally(() => notify("ui/notifications/initialized", {}));
})();
