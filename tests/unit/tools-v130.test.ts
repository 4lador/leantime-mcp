import { assertEquals } from "@std/assert";
import { LeantimeClient } from "../../src/leantime-client.ts";
import { createMockFetch } from "../helpers/mock-fetch.ts";
import { RPC_OK } from "../helpers/fixtures.ts";
import { registerAllTools } from "../../src/tools/mod.ts";

type ToolHandler = (params: Record<string, unknown>) => Promise<Record<string, unknown>>;

function createToolRegistry() {
  const tools = new Map<string, ToolHandler>();
  const mockServer = {
    tool: (name: string, _desc: string, _schema: unknown, handler: ToolHandler) => {
      tools.set(name, handler);
    },
  } as unknown as Parameters<typeof registerAllTools>[0];
  return { mockServer, tools };
}

function setup() {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);
  return { tools, calls };
}

const text = (r: unknown) =>
  ((r as Record<string, unknown>).content as Array<Record<string, unknown>>)[0].text as string;

// ---------------------------------------------------------------- destructive policy

Deno.test("destructive policy — default is ask: deletion without confirm is refused", async () => {
  Deno.env.delete("LEANTIME_MCP_DESTRUCTIVE_POLICY");
  const { tools, calls } = setup();

  const r = await tools.get("leantime_delete_ticket")!({ ticketId: "42" }) as Record<string, unknown>;
  assertEquals(r.isError, true);
  assertEquals(text(r).includes("Confirmation required"), true);
  assertEquals(calls.length, 0);
});

Deno.test("destructive policy — ask + confirm: true executes", async () => {
  Deno.env.delete("LEANTIME_MCP_DESTRUCTIVE_POLICY");
  const { tools, calls } = setup();

  const r = await tools.get("leantime_delete_ticket")!({ ticketId: "42", confirm: true }) as Record<string, unknown>;
  assertEquals(r.isError, undefined);
  assertEquals(calls[0].method, "leantime.rpc.tickets.delete");
  assertEquals(calls[0].params.id, "42");
});

Deno.test("destructive policy — deny refuses even with confirm", async () => {
  Deno.env.set("LEANTIME_MCP_DESTRUCTIVE_POLICY", "deny");
  try {
    const { tools, calls } = setup();
    const r = await tools.get("leantime_delete_ticket")!({ ticketId: "42", confirm: true }) as Record<string, unknown>;
    assertEquals(r.isError, true);
    assertEquals(text(r).includes("deny"), true);
    assertEquals(calls.length, 0);
  } finally {
    Deno.env.delete("LEANTIME_MCP_DESTRUCTIVE_POLICY");
  }
});

Deno.test("destructive policy — allow skips confirmation", async () => {
  Deno.env.set("LEANTIME_MCP_DESTRUCTIVE_POLICY", "allow");
  try {
    const { tools, calls } = setup();
    const r = await tools.get("leantime_delete_ticket")!({ ticketId: "42" }) as Record<string, unknown>;
    assertEquals(r.isError, undefined);
    assertEquals(calls[0].method, "leantime.rpc.tickets.delete");
  } finally {
    Deno.env.delete("LEANTIME_MCP_DESTRUCTIVE_POLICY");
  }
});

Deno.test("destructive policy — invalid value falls back to ask", async () => {
  Deno.env.set("LEANTIME_MCP_DESTRUCTIVE_POLICY", "nonsense");
  try {
    const { tools, calls } = setup();
    const r = await tools.get("leantime_delete_comment")!({ commentId: "10" }) as Record<string, unknown>;
    assertEquals(r.isError, true);
    assertEquals(calls.length, 0);
  } finally {
    Deno.env.delete("LEANTIME_MCP_DESTRUCTIVE_POLICY");
  }
});

Deno.test("destructive policy — every destructive tool is gated", async () => {
  Deno.env.delete("LEANTIME_MCP_DESTRUCTIVE_POLICY");
  const cases: [string, Record<string, unknown>, string][] = [
    ["leantime_delete_ticket", { ticketId: "42" }, "leantime.rpc.tickets.delete"],
    ["leantime_delete_milestone", { milestoneId: "22" }, "leantime.rpc.tickets.deleteMilestone"],
    ["leantime_delete_comment", { commentId: "10" }, "leantime.rpc.comments.deleteComment"],
    [
      "leantime_delete_timesheet_entry",
      { entryId: "50" },
      "leantime.rpc.timesheets.deleteTime",
    ],
  ];
  for (const [tool, args, method] of cases) {
    const { tools, calls } = setup();
    const refused = await tools.get(tool)!(args) as Record<string, unknown>;
    assertEquals(refused.isError, true, `${tool} must refuse without confirm`);
    assertEquals(calls.length, 0, `${tool} must not call the API without confirm`);
    const allowed = await tools.get(tool)!({ ...args, confirm: true }) as Record<string, unknown>;
    assertEquals(allowed.isError, undefined, `${tool} must accept with confirm`);
    assertEquals(calls[0].method, method, `${tool} must call ${method}`);
  }
});

// ---------------------------------------------------------------- comments

Deno.test("comments — list uses module/entityId params", async () => {
  const { tools, calls } = setup();
  await tools.get("leantime_list_comments")!({ ticketId: "1" });
  assertEquals(calls[0].method, "leantime.rpc.comments.getComments");
  assertEquals(calls[0].params.module, "ticket");
  assertEquals(calls[0].params.entityId, "1");
});

Deno.test("comments — add converts markdown and rebuilds entity", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_add_comment")!({ ticketId: "1", text: "C'est **fait**" });
  assertEquals(r.isError, undefined);
  const add = calls.find((c) => c.method === "leantime.rpc.comments.addComment")!;
  assertEquals(add.params.module, "ticket");
  assertEquals(add.params.entityId, "1");
  assertEquals((add.params.values as Record<string, unknown>).text, "<p>C'est <strong>fait</strong></p>");
  const entity = add.params.entity as Record<string, unknown>;
  assertEquals(entity.id, "1");
  assertEquals(typeof entity.headline, "string");
  assertEquals(typeof entity.type, "string");
});

Deno.test("comments — update converts markdown", async () => {
  const { tools, calls } = setup();
  await tools.get("leantime_update_comment")!({ commentId: "10", text: "Edit **ok**" });
  const edit = calls.find((c) => c.method === "leantime.rpc.comments.editComment")!;
  assertEquals(edit.params.id, "10");
  assertEquals((edit.params.values as Record<string, unknown>).text, "<p>Edit <strong>ok</strong></p>");
});

Deno.test("comments — add recovers from Leantime's post-insert notification crash", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch({
    "leantime.rpc.comments.addComment": () => {
      throw new Error("Leantime API error: 500 Server error — Attempt to read property on array");
    },
    "leantime.rpc.comments.getComments": () =>
      RPC_OK([{ id: "12", moduleId: "1", text: "<p>Malgré <strong>le crash</strong></p>" }]),
  });
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const r = await tools.get("leantime_add_comment")!({ ticketId: "1", text: "Malgré **le crash**" });
  assertEquals(r.isError, undefined);
  const parsed = JSON.parse(text(r));
  assertEquals(parsed.ok, true);
});

// ---------------------------------------------------------------- timesheets

Deno.test("timesheets — log_time defaults (add mode, GENERAL_BILLABLE, today)", async () => {
  const { tools, calls } = setup();
  await tools.get("leantime_log_time")!({ ticketId: "1", hours: 1.5 });
  const call = calls.find((c) => c.method === "leantime.rpc.timesheets.logTime")!;
  assertEquals(call.params.ticketId, "1");
  const params = call.params.params as Record<string, unknown>;
  assertEquals(params.kind, "GENERAL_BILLABLE");
  assertEquals(params.hours, 1.5);
  assertEquals(typeof params.date, "string");
});

Deno.test("timesheets — log_time mode set uses upsertTime", async () => {
  const { tools, calls } = setup();
  await tools.get("leantime_log_time")!({
    ticketId: "1",
    hours: 4,
    mode: "set",
    kind: "DEVELOPMENT",
    date: "2026-09-04",
    description: "impl",
  });
  const call = calls.find((c) => c.method === "leantime.rpc.timesheets.upsertTime")!;
  assertEquals(calls.some((c) => c.method === "leantime.rpc.timesheets.logTime"), false);
  const params = call.params.params as Record<string, unknown>;
  assertEquals(params.kind, "DEVELOPMENT");
  assertEquals(params.date, "2026-09-04");
  assertEquals(params.description, "impl");
});

Deno.test("timesheets — get_ticket_time merges total and byDate", async () => {
  const { tools } = setup();
  const r = await tools.get("leantime_get_ticket_time")!({ ticketId: "1" });
  const parsed = JSON.parse(text(r));
  assertEquals(parsed.totalHours, 2.5);
  assertEquals(Array.isArray(parsed.byDate), true);
});

Deno.test("timesheets — list sends date range with end-of-day expansion", async () => {
  const { tools, calls } = setup();
  await tools.get("leantime_list_timesheets")!({ dateFrom: "2026-09-01", dateTo: "2026-09-30" });
  const call = calls.find((c) => c.method === "leantime.rpc.timesheets.getAll")!;
  assertEquals(call.params.dateFrom, "2026-09-01");
  assertEquals(call.params.dateTo, "2026-09-30 23:59:59"); // inclusive end of day
});

// ---------------------------------------------------------------- milestones

Deno.test("milestones — create requires assignment", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_create_milestone")!({ projectId: "3", headline: "M1" }) as Record<string, unknown>;
  assertEquals(r.isError, true);
  assertEquals(text(r).includes("Assignment required"), true);
  assertEquals(calls.length, 1); // users.getAll only
});

Deno.test("milestones — create via addTicket with type milestone and markdown", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_create_milestone")!({
    projectId: "3",
    headline: "M1",
    description: "Objectif **v1**",
    editorId: "1",
    dateToFinish: "2026-10-01",
  });
  assertEquals(r.isError, undefined);
  const call = calls.find((c) => c.method === "leantime.rpc.tickets.addTicket")!;
  const values = call.params.values as Record<string, unknown>;
  assertEquals(values.type, "milestone");
  assertEquals(values.description, "<p>Objectif <strong>v1</strong></p>");
  assertEquals(values.editorId, "1");
  assertEquals(values.dateToFinish, "2026-10-01");
});

Deno.test("milestones — update uses tickets.patch (session-safe)", async () => {
  const { tools, calls } = setup();
  await tools.get("leantime_update_milestone")!({ milestoneId: "22", headline: "Renamed" });
  const call = calls.find((c) => c.method === "leantime.rpc.tickets.patch")!;
  assertEquals(call.params.id, "22");
  assertEquals((call.params.params as Record<string, unknown>).headline, "Renamed");
});

Deno.test("milestones — progress computed client-side (union param not RPC-castable)", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_get_milestone_progress")!({ milestoneId: "22" });
  const parsed = JSON.parse(text(r));
  assertEquals(typeof parsed.percentDone, "number");
  assertEquals(typeof parsed.tickets, "number");
  const methods = calls.map((c) => c.method);
  assertEquals(methods.includes("leantime.rpc.tickets.getTicket"), true);
  assertEquals(methods.includes("leantime.rpc.tickets.getAll"), true);
  assertEquals(methods.includes("leantime.rpc.tickets.getStatusLabels"), true);
  const getAll = calls.find((c) => c.method === "leantime.rpc.tickets.getAll")!;
  assertEquals((getAll.params.searchCriteria as Record<string, unknown>).milestone, "22");
});

// ---------------------------------------------------------------- sprints

Deno.test("sprints — create passes projectId explicitly", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_create_sprint")!({
    projectId: "3",
    name: "Sprint X",
    startDate: "2026-09-01",
    endDate: "2026-09-14",
  });
  assertEquals(r.isError, undefined);
  const call = calls.find((c) => c.method === "leantime.rpc.sprints.addSprint")!;
  const params = call.params.params as Record<string, unknown>;
  assertEquals(params.projectId, "3");
  assertEquals(params.name, "Sprint X");
});

Deno.test("sprints — update fetches sprint and resends full field set", async () => {
  const { tools, calls } = setup();
  await tools.get("leantime_update_sprint")!({ sprintId: "1", name: "Renamed" });
  const getSprint = calls.find((c) => c.method === "leantime.rpc.sprints.getSprint")!;
  assertEquals(getSprint.params.id, "1");
  const edit = calls.find((c) => c.method === "leantime.rpc.sprints.editSprint")!;
  const params = edit.params.params as Record<string, unknown>;
  assertEquals(params.id, "1");
  assertEquals(params.projectId, "3"); // from fetched sprint — never session
  assertEquals(params.name, "Renamed");
  assertEquals(typeof params.startDate, "string");
  assertEquals(typeof params.endDate, "string");
});

Deno.test("sprints — get_current computes from dates", async () => {
  const { tools } = setup();
  const r = await tools.get("leantime_get_current_sprint")!({ projectId: "3" });
  const parsed = JSON.parse(text(r));
  // Sprint 2 (2026-09-01 → 2026-09-30) covers "today" in the fixture timeline
  assertEquals(String(parsed.current?.id ?? ""), "2");
});

// ---------------------------------------------------------------- projects & clients

Deno.test("projects — create sends values with markdown details", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_create_project")!({
    name: "New Project",
    clientId: "1",
    details: "Un **projet**",
  });
  assertEquals(r.isError, undefined);
  const call = calls.find((c) => c.method === "leantime.rpc.projects.addProject")!;
  const values = call.params.values as Record<string, unknown>;
  assertEquals(values.name, "New Project");
  assertEquals(values.clientId, "1");
  assertEquals(values.details, "<p>Un <strong>projet</strong></p>");
});

Deno.test("projects — update patches only provided fields", async () => {
  const { tools, calls } = setup();
  await tools.get("leantime_update_project")!({ projectId: "3", name: "Renamed" });
  const call = calls.find((c) => c.method === "leantime.rpc.projects.patch")!;
  assertEquals(call.params.id, "3");
  assertEquals((call.params.params as Record<string, unknown>).name, "Renamed");
});

Deno.test("projects — find by term", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_find_projects")!({ term: "vision" });
  const parsed = JSON.parse(text(r));
  assertEquals(parsed.length, 1);
  assertEquals(parsed[0].name, "Vision");
  assertEquals(parsed[0].id, "3"); // mangled "3-<modified>" id normalized
  const call = calls.find((c) => c.method === "leantime.rpc.projects.findProject")!;
  assertEquals(call.params.term, "vision");
});

Deno.test("projects — list_project_users maps to id/name", async () => {
  const { tools } = setup();
  const r = await tools.get("leantime_list_project_users")!({ projectId: "3" });
  const parsed = JSON.parse(text(r));
  assertEquals(parsed[0], { id: "1", name: "Alador" });
});

Deno.test("clients — list maps to id/name", async () => {
  const { tools } = setup();
  const r = await tools.get("leantime_list_clients")!({});
  const parsed = JSON.parse(text(r));
  assertEquals(parsed, [{ id: "1", name: "TestClient" }]);
});

// ---------------------------------------------------------------- tickets extras

Deno.test("tickets — list_subtasks", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_list_subtasks")!({ ticketId: "1" });
  const parsed = JSON.parse(text(r));
  assertEquals(parsed.length, 1);
  assertEquals(parsed[0].headline, "A subtask");
  assertEquals(calls[0].params.ticketId, "1");
});

Deno.test("tickets — my_tasks maps project param", async () => {
  const { tools, calls } = setup();
  await tools.get("leantime_my_tasks")!({ userId: "1", projectId: "3" });
  const call = calls.find((c) => c.method === "leantime.rpc.tickets.getAllOpenUserTickets")!;
  assertEquals(call.params.userId, "1");
  assertEquals(call.params.project, "3");
});

Deno.test("tickets — get_ticket_options merges the four label sets", async () => {
  const { tools } = setup();
  const r = await tools.get("leantime_get_ticket_options")!({ projectId: "3" });
  const parsed = JSON.parse(text(r));
  assertEquals(parsed.priorities["3"], "Medium");
  assertEquals(parsed.efforts["1"], "S");
  assertEquals(parsed.kanban["3"], "New");
  assertEquals(typeof parsed.types, "object");
});
