// Loading the report: from JSON injected into report.html, or from `tarafold serve`.
import type { Report } from "./schema";

/** A sample of the input data, column-major (written next to the report by the binary). */
export interface DataSample {
  columns: string[];
  values: (number | null)[][];
  rows_total: number;
  sampled: boolean;
}

export interface Loaded {
  report: Report;
  data: DataSample | null;
  /** Present when the page is served by `tarafold serve`. */
  api: Api | null;
}

/** Client for the local server; the token comes from the URL fragment (`#token=...`). */
export class Api {
  constructor(
    readonly base: string,
    readonly token: string,
  ) {}

  private headers(json: boolean): HeadersInit {
    const h: Record<string, string> = { Authorization: `Bearer ${this.token}` };
    if (json) h["Content-Type"] = "application/json";
    return h;
  }

  async get<T>(path: string): Promise<T> {
    const r = await fetch(`${this.base}${path}`, { headers: this.headers(false) });
    if (!r.ok) throw new Error(await errorText(r));
    return (await r.json()) as T;
  }

  async post<T>(path: string, body: unknown): Promise<T> {
    const r = await fetch(`${this.base}${path}`, {
      method: "POST",
      headers: this.headers(true),
      body: JSON.stringify(body),
    });
    if (!r.ok) throw new Error(await errorText(r));
    return (await r.json()) as T;
  }

  /** Server-sent progress events of a pipeline run (`EventSource` cannot send headers). */
  events(onMessage: (m: ProgressEvent) => void): () => void {
    const ctrl = new AbortController();
    void (async () => {
      try {
        const r = await fetch(`${this.base}/api/events`, {
          headers: this.headers(false),
          signal: ctrl.signal,
        });
        const reader = r.body?.getReader();
        if (!reader) return;
        const dec = new TextDecoder();
        let buf = "";
        for (;;) {
          const { value, done } = await reader.read();
          if (done) break;
          buf += dec.decode(value, { stream: true });
          let i: number;
          while ((i = buf.indexOf("\n\n")) >= 0) {
            const chunk = buf.slice(0, i);
            buf = buf.slice(i + 2);
            const line = chunk.split("\n").find((l) => l.startsWith("data:"));
            if (line) {
              try {
                onMessage(JSON.parse(line.slice(5)) as ProgressEvent);
              } catch {
                /* ignore malformed events */
              }
            }
          }
        }
      } catch {
        /* aborted or connection closed */
      }
    })();
    return () => ctrl.abort();
  }
}

export interface ProgressEvent {
  kind: "started" | "stage" | "finished" | "failed";
  message: string;
}

async function errorText(r: Response): Promise<string> {
  try {
    const j = (await r.json()) as { error?: string };
    return j.error ?? `${r.status} ${r.statusText}`;
  } catch {
    return `${r.status} ${r.statusText}`;
  }
}

function embedded<T>(id: string): T | null {
  const el = document.getElementById(id);
  if (!el || !el.textContent || !el.textContent.trim()) return null;
  try {
    return JSON.parse(el.textContent) as T;
  } catch {
    return null;
  }
}

/** Token from `#token=...`; removed from the address bar after reading. */
function tokenFromHash(): string | null {
  const m = /(?:^#|&)token=([0-9a-fA-F]+)/.exec(window.location.hash);
  if (!m) return sessionStorage.getItem("tarafold-token");
  sessionStorage.setItem("tarafold-token", m[1]!);
  history.replaceState(null, "", window.location.pathname + window.location.search);
  return m[1]!;
}

export async function loadInitial(): Promise<Loaded> {
  const report = embedded<Report>("tarafold-report");
  const data = embedded<DataSample>("tarafold-data");
  if (report) return { report, data, api: null };
  if (window.location.protocol === "http:" || window.location.protocol === "https:") {
    const token = tokenFromHash();
    if (token) {
      const api = new Api("", token);
      const [rep, sample] = await Promise.all([
        api.get<Report>("/api/report"),
        api.get<DataSample | null>("/api/data").catch(() => null),
      ]);
      return { report: rep, data: sample, api };
    }
  }
  throw new Error(
    "No report found. Open report.html written by `tarafold run`, or start `tarafold serve <dir>` and use the printed URL.",
  );
}
