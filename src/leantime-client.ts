import type { JsonRpcRequest, JsonRpcResponse, LeantimeStatusMap } from "./types.ts";

export type FetchFn = typeof globalThis.fetch;

export class LeantimeClient {
  private baseUrl: string;
  private apiKey: string;
  private fetchFn: FetchFn;
  private rpcId = 0;
  private statusCache = new Map<string, LeantimeStatusMap>();
  private userCache: { value: Record<string, unknown>[]; expires: number } | null = null;

  /** Cache TTL for the user list (users rarely change; Leantime's default API
   * rate limit is 10 req/min, so avoid refetching on every ticket creation). */
  private static USER_CACHE_TTL_MS = 5 * 60 * 1000;

  constructor(baseUrl: string, apiKey: string, fetchFn?: FetchFn) {
    this.baseUrl = baseUrl.replace(/\/+$/, "");
    this.apiKey = apiKey;
    this.fetchFn = fetchFn ?? globalThis.fetch;
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

    const response = await this.fetchFn(`${this.baseUrl}/api/jsonrpc`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-api-key": this.apiKey,
      },
      body: JSON.stringify(request),
    });

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
