import { api } from './api';
import type { Response } from './protocol';

export type LinkState = 'connecting' | 'open' | 'reconnecting' | 'exited';

type Pending = { resolve: (r: Response) => void; reject: (e: Error) => void };

/** A connection that has not opened by now is given up on (an unreachable address would otherwise hang for the browser's own, much longer, limit). */
const CONNECT_TIMEOUT = 5000;
/** Waits between attempts: quick at first (on a LAN a restart is over in a second), then every 4 seconds. */
const BACKOFF = [250, 500, 1000, 2000, 4000];
/** Nothing heard for this long on an open socket: ask something small, to find out whether it is still alive. */
const IDLE_PROBE = 20_000;
/** How long that question may take. */
const PROBE_TIMEOUT = 4000;

/** One WebSocket to one pi process, with request/response correlation and reconnect. */
export class Connection {
  link = $state<LinkState>('connecting');

  #ws: WebSocket | null = null;
  #pending = new Map<string, Pending>();
  #seq = 0;
  #retry = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #disposed = false;
  #hasOpened = false;
  #openTimer: ReturnType<typeof setTimeout> | undefined;
  #idleTimer: ReturnType<typeof setInterval> | undefined;
  #lastRx = performance.now();
  #probing = false;

  constructor(
    readonly runtimeId: string,
    private onRecord: (rec: any) => void,
    private onOpen: (isReconnect: boolean) => void,
  ) {
    this.#connect();
    // A phone that has been asleep, or a network that dropped without telling, leaves a socket that looks open and isn't.
    this.#idleTimer = setInterval(() => {
      if (!document.hidden && this.link === 'open' && performance.now() - this.#lastRx > IDLE_PROBE) void this.#probe();
    }, 5000);
  }

  #connect() {
    if (this.#disposed) return;
    const ws = new WebSocket(api.wsUrl(this.runtimeId));
    this.#ws = ws;
    clearTimeout(this.#openTimer);
    this.#openTimer = setTimeout(() => { if (ws.readyState !== WebSocket.OPEN) ws.close(); }, CONNECT_TIMEOUT);
    ws.onopen = () => {
      clearTimeout(this.#openTimer);
      this.#lastRx = performance.now();
      this.#retry = 0;
      this.link = 'open';
      const again = this.#hasOpened;
      this.#hasOpened = true;
      this.onOpen(again);
    };
    ws.onmessage = (e) => {
      this.#lastRx = performance.now();
      let rec: any;
      try { rec = JSON.parse(e.data); } catch { return; }
      if (rec.type === 'response' && rec.id && this.#pending.has(rec.id)) {
        const p = this.#pending.get(rec.id)!;
        this.#pending.delete(rec.id);
        p.resolve(rec);
        return;
      }
      if (rec.type === 'oven_exit') this.link = 'exited';
      this.onRecord(rec);
    };
    ws.onclose = () => {
      if (ws !== this.#ws) return; // an old socket that was already replaced
      clearTimeout(this.#openTimer);
      this.#failPending(new Error('connection closed'));
      if (this.#disposed || this.link === 'exited') return;
      this.link = 'reconnecting';
      const delay = BACKOFF[Math.min(this.#retry++, BACKOFF.length - 1)];
      this.#timer = setTimeout(() => this.#connect(), delay);
    };
  }

  /** Give up on this socket now (without waiting for it to say it is closed) and open a new one. */
  #replace() {
    const old = this.#ws;
    if (old) { old.onopen = old.onmessage = old.onclose = null; try { old.close(); } catch { /* already gone */ } }
    clearTimeout(this.#timer);
    this.#failPending(new Error('connection closed'));
    if (this.#disposed || this.link === 'exited') return;
    this.link = 'reconnecting';
    this.#retry = 0;
    this.#connect();
  }

  async #probe() {
    if (this.#probing) return;
    this.#probing = true;
    const ws = this.#ws;
    try {
      await this.request({ type: 'get_state' }, PROBE_TIMEOUT);
    } catch {
      if (this.#ws === ws && !this.#disposed) this.#replace();
    } finally {
      this.#probing = false;
    }
  }

  /** The page came back to the front, or the network came back: make sure there is a live connection, now. */
  nudge() {
    if (this.#disposed || this.link === 'exited') return;
    const ws = this.#ws;
    if (!ws || ws.readyState === WebSocket.CLOSED || ws.readyState === WebSocket.CLOSING) return this.#replace();
    if (ws.readyState === WebSocket.OPEN) void this.#probe();
  }

  #failPending(err: Error) {
    for (const p of this.#pending.values()) p.reject(err);
    this.#pending.clear();
  }

  /** Send a command and wait for its response. Rejects on `success: false`. */
  async request<T = any>(cmd: Record<string, unknown>, timeoutMs?: number): Promise<T> {
    const id = `c${++this.#seq}`;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const res = await new Promise<Response>((resolve, reject) => {
      if (this.#ws?.readyState !== WebSocket.OPEN) return reject(new Error('not connected'));
      this.#pending.set(id, { resolve, reject });
      if (timeoutMs) timer = setTimeout(() => { this.#pending.delete(id); reject(new Error('timeout')); }, timeoutMs);
      this.#ws.send(JSON.stringify({ ...cmd, id }));
    }).finally(() => clearTimeout(timer));
    if (!res.success) throw new Error(res.error ?? `${res.command} failed`);
    return res.data as T;
  }

  /** Fire-and-forget (extension UI answers have no response). */
  send(rec: Record<string, unknown>) {
    if (this.#ws?.readyState === WebSocket.OPEN) this.#ws.send(JSON.stringify(rec));
  }

  dispose() {
    this.#disposed = true;
    clearTimeout(this.#timer);
    clearTimeout(this.#openTimer);
    clearInterval(this.#idleTimer);
    this.#failPending(new Error('disposed'));
    this.#ws?.close();
  }
}
