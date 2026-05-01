import type { LeantimeStatusMap } from "../../src/types.ts";
import { STATUS_MAP, TICKETS, PROJECTS, MILESTONES, SPRINTS, RPC_OK, RPC_ERROR } from "./fixtures.ts";

type RpcHandler = (method: string, params: Record<string, unknown>) => unknown;

export function createMockFetch(handler?: Partial<Record<string, RpcHandler>>) {
  const calls: { method: string; params: Record<string, unknown> }[] = [];

  const defaultHandlers: Record<string, RpcHandler> = {
    "leantime.rpc.Projects.getAll": () => RPC_OK(PROJECTS),
    "leantime.rpc.projects.getProject": () => RPC_OK(PROJECTS[0]),
    "leantime.rpc.projects.getProjectProgress": () => RPC_OK({ percent: 50, totalTickets: 10, doneTickets: 5, openTickets: 5 }),
    "leantime.rpc.tickets.getAll": (_method, params) => {
      if (params.type === "milestone") return RPC_OK(MILESTONES);
      return RPC_OK(TICKETS);
    },
    "leantime.rpc.tickets.getTicket": () => RPC_OK(TICKETS[0]),
    "leantime.rpc.tickets.getStatusLabels": () => RPC_OK(STATUS_MAP),
    "leantime.rpc.tickets.addTicket": (_method, params) => RPC_OK({ id: 999, ...params }),
    "leantime.rpc.tickets.updateTicket": (_method, params) => RPC_OK({ success: true, ...params }),
    "leantime.rpc.tickets.getTicketTypes": () => RPC_OK({ task: "Task", story: "Story", bug: "Bug" }),
    "leantime.rpc.sprints.getAllSprints": () => RPC_OK(SPRINTS),
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
