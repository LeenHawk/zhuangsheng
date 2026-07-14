export const pluginFrameSource = `<!doctype html>
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' blob:; worker-src blob:; connect-src 'none'; img-src 'none'; style-src 'none'; font-src 'none'; media-src 'none'; object-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'">
<script>(() => {
  let worker = null;
  let token = null;
  const send = (message) => parent.postMessage({ channel: "zhuangsheng-plugin-v1", token, ...message }, "*");
  const workerMain = () => {
    let loaded = null;
    addEventListener("message", async (event) => {
      const value = event.data;
      if (value.kind === "load") {
        let url = null;
        try {
          url = URL.createObjectURL(new Blob([value.code], { type: "text/javascript" }));
          loaded = await import(url);
          if (typeof loaded.render !== "function") throw new Error("plugin must export render(request)");
          postMessage({ kind: "loaded" });
        } catch (error) {
          postMessage({ kind: "loaded", error: error instanceof Error ? error.message : "plugin load failed" });
        } finally { if (url) URL.revokeObjectURL(url); }
        return;
      }
      if (value.kind !== "render" || !loaded) return;
      try {
        const nodes = await loaded.render(value.request);
        postMessage({ kind: "result", requestId: value.requestId, nodes });
      } catch (error) {
        postMessage({ kind: "result", requestId: value.requestId, error: error instanceof Error ? error.message : "plugin render failed" });
      }
    });
  };
  addEventListener("message", (event) => {
    const value = event.data;
    if (!value || value.channel !== "zhuangsheng-host-v1") return;
    if (value.kind === "dispose") { worker?.terminate(); worker = null; return; }
    if (value.kind === "load") {
      token = value.token;
      worker?.terminate();
      const source = "(" + workerMain.toString() + ")()";
      const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
      worker = new Worker(url);
      URL.revokeObjectURL(url);
      worker.addEventListener("message", (message) => send(message.data));
      worker.addEventListener("error", () => send({ kind: "worker_error" }));
      worker.postMessage({ kind: "load", code: value.code });
      return;
    }
    if (value.token === token) worker?.postMessage(value);
  });
  parent.postMessage({ channel: "zhuangsheng-plugin-v1", kind: "ready" }, "*");
})();</script>`;
