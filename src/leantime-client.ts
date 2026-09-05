import type { JsonRpcRequest, JsonRpcResponse, LeantimeStatusMap } from "./types.ts";

export type FetchFn = typeof globalThis.fetch;

/** Maximum retries on 429 rate limit before giving up with a clear error. */
const MAX_429_RETRIES = 5;

/** Maximum retries on transient network errors (502/503/504). */
const MAX_NETWORK_RETRIES = 2;

/** Leantime's default API rate limit (req/min) — used as a conservative
 * fallback when the server doesn't provide headers. */
const DEFAULT_RATE_LIMIT_PER_MIN = 10;

export class LeantimeClient {
  private baseUrl: string;
  private apiKey: string;
  private fetchFn: FetchFn;
  private rpcId = 0;
  private statusCache = new Map<string, LeantimeStatusMap>();
  private userCache: { value: Record<string, unknown>[]; expires: number } | null = null;

  /** Rate limit discovered from a 429 response's X-RateLimit-Limit header.
   * Cached for the client's lifetime — used to calculate inter-request delays. */
  private discoveredRateLimit: number | null = null;

  /** Cache TTL for the user list (users rarely change; Leantime's default API
   * rate limit is 10 req/min, so avoid refetching on every ticket creation). */
  private static USER_CACHE_TTL_MS = 5 * 60 * 1000;

  constructor(baseUrl: string, apiKey: string, fetchFn?: FetchFn) {
    this.baseUrl = baseUrl.replace(/\/+$/, "");
    this.apiKey = apiKey;
    this.fetchFn = fetchFn ?? globalThis.fetch;
  }

  /**
   * Parse Retry-After / X-RateLimit-Retry-After headers into milliseconds.
   * Handles both "seconds" format and HTTP-date format.
   * Returns null when no usable header is present.
   */
  private parseRetryAfter(response: Response): number | null {
    const ra = response.headers.get("Retry-After");
    if (ra) {
      const secs = parseInt(ra, 10);
      if (!isNaN(secs) && secs >= 0) return secs * 1000;
      const date = new Date(ra);
      if (!isNaN(date.getTime())) {
        return Math.max(0, date.getTime() - Date.now());
      }
    }
    const xlra = response.headers.get("X-RateLimit-Retry-After");
    if (xlra) {
      const secs = parseInt(xlra, 10);
      if (!isNaN(secs) && secs >= 0) return secs * 1000;
    }
    return null;
  }

  /**
   * Discover and cache the rate limit from a 429 response.
   * Leantime always sends X-RateLimit-Limit on 429 responses.
   */
  private discoverRateLimit(response: Response): void {
    if (this.discoveredRateLimit !== null) return;
    const limit = response.headers.get("X-RateLimit-Limit");
    if (limit) {
      const parsed = parseInt(limit, 10);
      if (!isNaN(parsed) && parsed > 0) {
        this.discoveredRateLimit = parsed;
      }
    }
  }

  /**
   * Calculate the inter-request delay for the current rate limit.
   * Uses the discovered limit if available, otherwise a conservative default
   * (10 req/min = 6s between requests).
   */
  private get interRequestDelay(): number {
    const limit = this.discoveredRateLimit ?? DEFAULT_RATE_LIMIT_PER_MIN;
    return Math.ceil(60000 / limit); // 60s / limit, rounded up
  }

  async call<T = unknown>(
    method: string,
    params?: Record<string, unknown>,
  ): Promise<T> {
    const request: JsonRpcRequest = {
      jsonrpc: "2.0",
      method: `leantime.rpc.${method}`,
      params: params ?? {},
      id: ++this.rpcId,
    };

    let networkRetries = 0;
    let last429Delay = 0;

    for (
      let attempt = 0;
      attempt <= MAX_429_RETRIES + MAX_NETWORK_RETRIES;
      attempt++
    ) {
      const response = await this.fetchFn(`${this.baseUrl}/api/jsonrpc`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "x-api-key": this.apiKey,
        },
        body: JSON.stringify(request),
      });

      // ---- Transient network errors (502/503/504): short retry ----
      if ([502, 503, 504].includes(response.status)) {
        if (networkRetries < MAX_NETWORK_RETRIES) {
          networkRetries++;
          const delay = networkRetries === 1 ? 500 : 1000;
          await new Promise((r) => setTimeout(r, delay));
          continue;
        }
        throw new Error(
          `Leantime API error: ${response.status} ${response.statusText} ` +
          `(retried ${MAX_NETWORK_RETRIES} times)`,
        );
      }

      // ---- 429 rate limit: adaptive retry ----
      if (response.status === 429) {
        this.discoverRateLimit(response);

        if (attempt < MAX_429_RETRIES) {
          // Priority: Retry-After header > discovered rate limit delay > conservative default
          const headerDelay = this.parseRetryAfter(response);
          const rateLimitDelay = this.interRequestDelay;
          last429Delay = headerDelay ?? Math.max(rateLimitDelay, 1000);
          await new Promise((r) => setTimeout(r, last429Delay));
          continue;
        }
        const limit = this.discoveredRateLimit ?? DEFAULT_RATE_LIMIT_PER_MIN;
        throw new Error(
          `Rate limit exhausted after ${MAX_429_RETRIES} retries` +
          (last429Delay > 0 ? ` (waited ~${Math.round(last429Delay / 1000)}s per retry)` : "") +
          ` — the instance allows ~${limit} req/min. ` +
          `Wait ~60 seconds or reduce the batch size.`,
        );
      }

      if (!response.ok) {
        throw new Error(
          `Leantime API error: ${response.status} ${response.statusText}`,
        );
      }

      const json: JsonRpcResponse<T> = await response.json();

      if (json.error) {
        throw new Error(
          `Leantime RPC error [${json.error.code}]: ${json.error.message}${json.error.data ? ` — ${json.error.data}` : ""}`,
        );
      }

      return json.result as T;
    }

    // Unreachable (loop always returns or throws), but TypeScript needs it
    throw new Error("LeantimeClient: unreachable state");
  }

  async getStatusMap(projectId: string): Promise<LeantimeStatusMap> {
    const cached = this.statusCache.get(projectId);
    if (cached) return cached;

    const result = await this.call<LeantimeStatusMap>(
      "tickets.getStatusLabels",
      { projectId },
    );
    this.statusCache.set(projectId, result);
    return result;
  }

  async getUsers(): Promise<Record<string, unknown>[]> {
    if (this.userCache && this.userCache.expires > Date.now()) {
      return this.userCache.value;
    }
    const value = await this.call<Record<string, unknown>[]>("users.getAll");
    this.userCache = { value, expires: Date.now() + LeantimeClient.USER_CACHE_TTL_MS };
    return value;
  }

  async enrichWithStatuses<T extends Record<string, unknown>>(
    items: T[],
    projectId: string,
  ): Promise<T[]> {
    const statusMap = await this.getStatusMap(projectId);
    return items.map((item) => enrichItem(item, statusMap));
  }

  async enrichSingleWithStatuses<T extends Record<string, unknown>>(
    item: T,
    projectId: string,
  ): Promise<T> {
    const statusMap = await this.getStatusMap(projectId);
    return enrichItem(item, statusMap);
  }
}

export function enrichItem<T extends Record<string, unknown>>(
  item: T,
  statusMap: LeantimeStatusMap,
): T {
  const statusKey = String(item.status);
  const statusInfo = statusMap[statusKey];
  if (!statusInfo) return item;
  return {
    ...item,
    statusLabel: statusInfo.name,
    statusType: statusInfo.statusType,
    statusColor: statusInfo.class,
  };
}
