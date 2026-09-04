import type { LeantimeStatusMap } from "../../src/types.ts";
import { STATUS_MAP, TICKETS, PROJECTS, MILESTONES, SPRINTS, USERS, RPC_OK, RPC_ERROR } from "./fixtures.ts";

type RpcHandler = (method: string, params: Record<string, unknown>) => unknown;

export function createMockFetch(handler?: Partial<Record<string, RpcHandler>>) {
  const calls: { method: string; params: Record<string, unknown> }[] = [];

  const defaultHandlers: Record<string, RpcHandler> = {
    "leantime.rpc.Projects.getAll": () => RPC_OK(PROJECTS),
    "leantime.rpc.projects.getProject": () => RPC_OK(PROJECTS[0]),
    "leantime.rpc.projects.getProjectProgress": () => RPC_OK({ percent: 50, totalTickets: 10, doneTickets: 5, openTickets: 5 }),
    // Mirrors Leantime's real behavior: filters ONLY apply inside
    // `searchCriteria` — flat params are silently ignored.
    "leantime.rpc.tickets.getAll": (_method, params) => {
      const sc = (params.searchCriteria ?? {}) as Record<string, unknown>;
      let pool: Record<string, unknown>[] = [...TICKETS, ...MILESTONES];
      if (sc.currentProject !== undefined && sc.currentProject !== "") {
        pool = pool.filter((t) => String(t.projectId) === String(sc.currentProject));
      }
      if (sc.type !== undefined && sc.type !== "") {
        const types = String(sc.type).split(",").map((t) => t.trim().toLowerCase());
        pool = pool.filter((t) => types.includes(String(t.type).toLowerCase()));
      }
      if (sc.status !== undefined && sc.status !== "" && sc.status !== "all") {
        const statuses = String(sc.status).split(",").map((s) => s.trim());
        pool = pool.filter((t) => statuses.includes(String(t.status)));
      }
      if (sc.milestone !== undefined && sc.milestone !== "") {
        const ids = String(sc.milestone).split(",").map((m) => m.trim());
        pool = pool.filter((t) => ids.includes(String(t.milestoneid ?? "")));
      }
      if (sc.users !== undefined && sc.users !== "") {
        const users = String(sc.users).split(",").map((u) => u.trim());
        pool = pool.filter((t) => users.includes(String(t.editorId ?? "")));
      }
      if (sc.term !== undefined && sc.term !== "") {
        const term = String(sc.term).toLowerCase();
        pool = pool.filter((t) =>
          String(t.headline).toLowerCase().includes(term) ||
          String(t.description ?? "").toLowerCase().includes(term)
        );
      }
      return RPC_OK(pool);
    },
    "leantime.rpc.tickets.getTicket": () => RPC_OK(TICKETS[0]),
    "leantime.rpc.tickets.getStatusLabels": () => RPC_OK(STATUS_MAP),
    "leantime.rpc.tickets.addTicket": () => RPC_OK([999]),
    "leantime.rpc.tickets.updateTicket": (_method, params) => RPC_OK({ success: true, ...params }),
    "leantime.rpc.tickets.patch": () => RPC_OK(true),
    "leantime.rpc.tickets.getTicketTypes": () => RPC_OK({ task: "Task", story: "Story", bug: "Bug" }),
    "leantime.rpc.sprints.getAllSprints": () => RPC_OK(SPRINTS),
    "leantime.rpc.users.getAll": () => RPC_OK(USERS),
  };

  const allHandlers = { ...defaultHandlers, ...handler };

  const mockFetch = async (url: string | URL, init?: RequestInit): Promise<Response> => {
    const body = JSON.parse(init?.body as string);
    calls.push({ method: body.method, params: body.params });

    const rpcHandler = allHandlers[body.method];
    if (rpcHandler) {
      const result = rpcHandler(body.method, body.params || {});
      return new Response(JSON.stringify(result), { status: 200, headers: { "Content-Type": "application/json" } });
    }

    return new Response(JSON.stringify(RPC_ERROR(-32601, "Method not found")), { status: 200, headers: { "Content-Type": "application/json" } });
  };

  return { fetch: mockFetch as typeof globalThis.fetch, calls };
}
