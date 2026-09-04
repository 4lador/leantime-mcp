import type { LeantimeStatusMap } from "../../src/types.ts";
import { STATUS_MAP, TICKETS, PROJECTS, MILESTONES, SPRINTS, USERS, CLIENTS, COMMENTS, TIMESHEETS, RPC_OK, RPC_ERROR } from "./fixtures.ts";

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
    "leantime.rpc.Api.getAPIKeys": () =>
      RPC_OK([{ id: 3, username: "testu", role: "20", firstname: "MCP" }]),
    "leantime.rpc.Api.createAPIKey": () =>
      RPC_OK({
        id: 5,
        user: "nEwUsEr12345678901234567890ab",
        passwordClean: "pAsSwOrD12345678901234567890ab",
      }),
    "leantime.rpc.Projects.getProjectsAssignedToUser": () => RPC_OK(PROJECTS),
    "leantime.rpc.Projects.editUserProjectRelations": () => RPC_OK(true),
    "leantime.rpc.Clients.getAll": () => RPC_OK(CLIENTS),
    "leantime.rpc.comments.getComments": (_m, params) =>
      RPC_OK(COMMENTS.filter((c) => String(c.moduleId) === String(params.entityId))),
    "leantime.rpc.comments.addComment": () => RPC_OK(true),
    "leantime.rpc.comments.editComment": () => RPC_OK(true),
    "leantime.rpc.comments.deleteComment": () => RPC_OK(true),
    "leantime.rpc.timesheets.logTime": () => RPC_OK(true),
    "leantime.rpc.timesheets.upsertTime": () => RPC_OK(true),
    "leantime.rpc.timesheets.getSumLoggedHoursForTicket": () => RPC_OK(2.5),
    "leantime.rpc.timesheets.getLoggedHoursForTicketByDate": () =>
      RPC_OK([{ workDate: "2026-09-04", hours: 2.5 }]),
    "leantime.rpc.timesheets.getAll": (_m, params) =>
      RPC_OK(
        TIMESHEETS.filter(
          (t) => params.projectId === undefined || String(t.ticketId) !== "999",
        ),
      ),
    "leantime.rpc.timesheets.deleteTime": () => RPC_OK(true),
    "leantime.rpc.sprints.getSprint": () => RPC_OK(SPRINTS[0]),
    "leantime.rpc.sprints.addSprint": () => RPC_OK(2),
    "leantime.rpc.sprints.editSprint": (_m, params) =>
      RPC_OK({ ...(params.params as Record<string, unknown>) }),
    "leantime.rpc.projects.addProject": () => RPC_OK(5),
    "leantime.rpc.projects.patch": () => RPC_OK(true),
    "leantime.rpc.projects.findProject": (_m, params) =>
      RPC_OK(
        PROJECTS
          .filter((p) => p.name.toLowerCase().includes(String(params.term ?? "").toLowerCase()))
          // Real API mangles ids into "id-modified"
          .map((p) => ({ ...p, id: `${p.id}-2026-09-04 00:00:00` })),
      ),
    "leantime.rpc.projects.getUsersAssignedToProject": () => RPC_OK(USERS),
    "leantime.rpc.tickets.getMilestoneProgress": () => RPC_OK(50.0),
    "leantime.rpc.tickets.delete": () => RPC_OK(true),
    "leantime.rpc.tickets.deleteMilestone": () => RPC_OK(true),
    "leantime.rpc.tickets.getAllSubtasks": (_m, params) =>
      RPC_OK(TICKETS.filter((t) => String(t.dependingTicketId ?? "") === String(params.ticketId))),
    "leantime.rpc.tickets.getAllOpenUserTickets": (_m, params) =>
      RPC_OK(
        TICKETS.filter(
          (t) =>
            Number(t.status) !== 0 &&
            (params.project === undefined || String(t.projectId) === String(params.project)),
        ),
      ),
    "leantime.rpc.tickets.getPriorityLabels": () =>
      RPC_OK({ "1": "Low", "3": "Medium", "5": "High" }),
    "leantime.rpc.tickets.getEffortLabels": () =>
      RPC_OK({ "0": "?", "1": "S", "2": "M" }),
    "leantime.rpc.tickets.getKanbanColumns": () =>
      RPC_OK({ "3": "New", "4": "In Progress", "0": "Done" }),
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
