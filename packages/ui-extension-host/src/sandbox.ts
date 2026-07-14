import { pluginFrameSource } from "./iframe-source";
import type { PluginRenderRequest, UiNode } from "./types";
import { validateUiNodes } from "./validation";

interface Pending {
  resolve: (value: UiNode[]) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

export class UiExtensionSandbox {
  private readonly frame = document.createElement("iframe");
  private readonly token = crypto.randomUUID();
  private readonly pending = new Map<string, Pending>();
  private readyResolve?: () => void;
  private isReady = false;
  private loadResolve?: (error?: string) => void;
  private disposed = false;

  private constructor() {
    this.frame.hidden = true;
    this.frame.tabIndex = -1;
    this.frame.setAttribute("aria-hidden", "true");
    this.frame.setAttribute("sandbox", "allow-scripts");
    this.frame.srcdoc = pluginFrameSource;
    window.addEventListener("message", this.onMessage);
    document.body.append(this.frame);
  }

  static async create(code: string): Promise<UiExtensionSandbox> {
    const sandbox = new UiExtensionSandbox();
    try {
      await sandbox.waitUntilReady();
      await sandbox.load(code);
      return sandbox;
    } catch (error) {
      sandbox.dispose();
      throw error;
    }
  }

  render(request: PluginRenderRequest, timeoutMs = 1_500): Promise<UiNode[]> {
    if (this.disposed) return Promise.reject(new Error("plugin sandbox is disposed"));
    const requestId = crypto.randomUUID();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(requestId);
        reject(new Error("plugin renderer timed out"));
        this.dispose();
      }, timeoutMs);
      this.pending.set(requestId, { resolve, reject, timer });
      this.post({ kind: "render", token: this.token, requestId, request });
    });
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    window.removeEventListener("message", this.onMessage);
    this.post({ kind: "dispose", token: this.token });
    this.frame.remove();
    for (const request of this.pending.values()) {
      clearTimeout(request.timer);
      request.reject(new Error("plugin sandbox was disposed"));
    }
    this.pending.clear();
  }

  private waitUntilReady(): Promise<void> {
    if (this.isReady) return Promise.resolve();
    return deadline((resolve) => { this.readyResolve = resolve; }, "plugin sandbox did not start");
  }

  private load(code: string): Promise<void> {
    const result = deadline<string | undefined>((resolve) => { this.loadResolve = resolve; }, "plugin sandbox did not load");
    this.post({ kind: "load", token: this.token, code });
    return result.then((error) => { if (error) throw new Error(error); });
  }

  private post(value: unknown): void {
    this.frame.contentWindow?.postMessage({ channel: "zhuangsheng-host-v1", ...(value as object) }, "*");
  }

  private readonly onMessage = (event: MessageEvent) => {
    if (event.source !== this.frame.contentWindow) return;
    const value = event.data as Record<string, unknown> | null;
    if (!value || value.channel !== "zhuangsheng-plugin-v1") return;
    if (value.kind === "ready") {
      this.isReady = true; this.readyResolve?.(); this.readyResolve = undefined; return;
    }
    if (value.token !== this.token) return;
    if (value.kind === "loaded") { this.loadResolve?.(typeof value.error === "string" ? value.error : undefined); this.loadResolve = undefined; return; }
    if (value.kind !== "result" || typeof value.requestId !== "string") return;
    const pending = this.pending.get(value.requestId);
    if (!pending) return;
    this.pending.delete(value.requestId); clearTimeout(pending.timer);
    if (typeof value.error === "string") pending.reject(new Error(value.error));
    else { try { pending.resolve(validateUiNodes(value.nodes)); } catch (error) { pending.reject(error as Error); } }
  };
}

function deadline<T>(register: (resolve: (value: T) => void) => void, message: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(message)), 3_000);
    register((value) => { clearTimeout(timer); resolve(value); });
  });
}
