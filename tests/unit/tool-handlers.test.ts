import { assertEquals } from "@std/assert";
import { LeantimeClient } from "../../src/leantime-client.ts";
import { createMockFetch } from "../helpers/mock-fetch.ts";
import { registerAllTools } from "../../src/tools/mod.ts";

type ToolHandler = (params: Record<string, unknown>) => Promise<unknown>;

function createToolRegistry() {
  const tools = new Map<string, ToolHandler>();
  const mockServer = {
    tool: (name: string, _desc: string, _schema: unknown, handler: ToolHandler) => {
      tools.set(name, handler);
    },
  } as unknown as Parameters<typeof registerAllTools>[0];
  return { mockServer, tools };
}

Deno.test("tool handler — leantime_list_projects returns JSON text", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_list_projects")!({}) as Record<string, unknown>;
  const content = result.content as Array<Record<string, unknown>>;
  assertEquals(content[0].type, "text");
  const parsed = JSON.parse(content[0].text as string);
  assertEquals(Array.isArray(parsed), true);
  assertEquals(parsed.length, 2);
  assertEquals(parsed[0].name, "Vision");
  assertEquals(result.isError, undefined);
});

Deno.test("tool handler — leantime_list_tickets enriches with statuses", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_list_tickets")!({ projectId: "3" }) as Record<string, unknown>;
  const parsed = JSON.parse((result.content as Array<Record<string, unknown>>)[0].text as string);
  // Leantime returns milestones too unless filtered by type: 6 tasks + 2 milestones
  assertEquals(parsed.length, 8);
  assertEquals(parsed[0].statusLabel, "Terminé");
  assertEquals(parsed[0].statusType, "DONE");
});

Deno.test("tool handler — list_tickets with type filter excludes milestones", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_list_tickets")!({ projectId: "3", type: "task" }) as Record<string, unknown>;
  const parsed = JSON.parse((result.content as Array<Record<string, unknown>>)[0].text as string);
  assertEquals(parsed.length, 6);
});

Deno.test("tool handler — list_tickets is scoped to the requested project (regression)", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_list_tickets")!({ projectId: "4" }) as Record<string, unknown>;
  const parsed = JSON.parse((result.content as Array<Record<string, unknown>>)[0].text as string);
  assertEquals(parsed.length, 1);
  assertEquals(parsed[0].headline, "Other project task");
  assertEquals(parsed.every((t: Record<string, unknown>) => String(t.projectId) === "4"), true);
});

Deno.test("tool handler — list_tickets sends filters inside searchCriteria", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  await tools.get("leantime_list_tickets")!({
    projectId: "3",
    status: "3",
    milestoneId: "22",
    sprintId: "1",
    userId: "2",
    type: "task",
    search: "test",
  });

  const sc = calls[0].params.searchCriteria as Record<string, unknown>;
  assertEquals(sc.currentProject, "3");
  assertEquals(sc.status, "3");
  assertEquals(sc.milestone, "22");
  assertEquals(sc.sprint, "1");
  assertEquals(sc.users, "2");
  assertEquals(sc.type, "task");
  assertEquals(sc.term, "test");
});

Deno.test("tool handler — leantime_get_ticket enriches single ticket", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_get_ticket")!({ projectId: "3", ticketId: "1" }) as Record<string, unknown>;
  const parsed = JSON.parse((result.content as Array<Record<string, unknown>>)[0].text as string);
  assertEquals(parsed.statusLabel, "Terminé");
  assertEquals(parsed.statusType, "DONE");
});

Deno.test("tool handler — leantime_create_ticket returns result", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_create_ticket")!({
    projectId: "3",
    headline: "New task",
    type: "task",
    editorId: "1",
  }) as Record<string, unknown>;
  const content = (result.content as Array<Record<string, unknown>>)[0];
  assertEquals(content.type, "text");
  assertEquals(calls[0].method, "leantime.rpc.users.getAll");
  assertEquals(calls[1].method, "leantime.rpc.tickets.addTicket");
  const values = calls[1].params.values as Record<string, unknown>;
  assertEquals(values.headline, "New task");
  assertEquals(values.editorId, "1");
});

Deno.test("tool handler — create without assignment is rejected", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_create_ticket")!({
    projectId: "3",
    headline: "New task",
  }) as Record<string, unknown>;

  assertEquals(result.isError, true);
  const text = (result.content as Array<Record<string, unknown>>)[0].text as string;
  assertEquals(text.includes("Assignment required"), true);
  assertEquals(text.includes("1 (Alador)"), true);
  // No ticket was created
  assertEquals(calls.some((c) => c.method === "leantime.rpc.tickets.addTicket"), false);
});

Deno.test("tool handler — create with unassigned flag succeeds without editorId", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_create_ticket")!({
    projectId: "3",
    headline: "New task",
    unassigned: true,
  }) as Record<string, unknown>;

  assertEquals(result.isError, undefined);
  const addCall = calls.find((c) => c.method === "leantime.rpc.tickets.addTicket")!;
  assertEquals((addCall.params.values as Record<string, unknown>).editorId, undefined);
});

Deno.test("tool handler — create with unknown editorId is rejected", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_create_ticket")!({
    projectId: "3",
    headline: "New task",
    editorId: "99",
  }) as Record<string, unknown>;

  assertEquals(result.isError, true);
  const text = (result.content as Array<Record<string, unknown>>)[0].text as string;
  assertEquals(text.includes(`editorId "99" does not exist`), true);
  assertEquals(calls.some((c) => c.method === "leantime.rpc.tickets.addTicket"), false);
});

Deno.test("tool handler — create converts markdown description to html", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  await tools.get("leantime_create_ticket")!({
    projectId: "3",
    headline: "Doc task",
    description: "## Objectif\n\nFaire **le tour** du sujet :\n- [ ] point un\n- point deux",
    unassigned: true,
  });

  const addCall = calls.find((c) => c.method === "leantime.rpc.tickets.addTicket")!;
  assertEquals(
    (addCall.params.values as Record<string, unknown>).description,
    "<h2>Objectif</h2><p>Faire <strong>le tour</strong> du sujet :</p>" +
      '<ul data-type="taskList"><li data-type="taskItem" data-checked="false"><p>point un</p></li>' +
      '<li data-type="taskItem" data-checked="false"><p>point deux</p></li></ul>',
  );
});

Deno.test("tool handler — update converts markdown description to html", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  await tools.get("leantime_update_ticket")!({
    ticketId: "42",
    description: "Nouveau **contenu**",
  });

  const updateCall = calls.find((c) => c.method === "leantime.rpc.tickets.patch")!;
  assertEquals(
    (updateCall.params.params as Record<string, unknown>).description,
    "<p>Nouveau <strong>contenu</strong></p>",
  );
});

Deno.test("tool handler — update with unknown editorId is rejected", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_update_ticket")!({
    ticketId: "42",
    editorId: "77",
  }) as Record<string, unknown>;

  assertEquals(result.isError, true);
  assertEquals(calls.some((c) => c.method === "leantime.rpc.tickets.patch"), false);
});

Deno.test("tool handler — leantime_list_users returns id and name", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_list_users")!({}) as Record<string, unknown>;
  const parsed = JSON.parse((result.content as Array<Record<string, unknown>>)[0].text as string);
  assertEquals(parsed.length, 2);
  assertEquals(parsed[0], { id: "1", name: "Alador" });
  assertEquals(parsed[1], { id: "2", name: "LM" });
});

Deno.test("tool handler — leantime_update_ticket patches only provided fields", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  await tools.get("leantime_update_ticket")!({
    ticketId: "42",
    milestoneId: "22",
    planHours: 8,
  });
  const patchCall = calls.find((c) => c.method === "leantime.rpc.tickets.patch")!;
  assertEquals(patchCall.params.id, "42");
  const params = patchCall.params.params as Record<string, unknown>;
  // Only the provided fields are sent — nothing else is wiped
  assertEquals(Object.keys(params).sort(), ["milestoneid", "planHours"]);
  assertEquals(params.milestoneid, "22");
  assertEquals(params.planHours, 8);
});

Deno.test("tool handler — update with no fields is rejected", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_update_ticket")!({ ticketId: "42" }) as Record<string, unknown>;
  assertEquals(result.isError, true);
  assertEquals(calls.length, 0);
});

Deno.test("tool handler — leantime_get_statuses returns status map", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_get_statuses")!({ projectId: "3" }) as Record<string, unknown>;
  const parsed = JSON.parse((result.content as Array<Record<string, unknown>>)[0].text as string);
  assertEquals(parsed["0"].name, "Terminé");
  assertEquals(parsed["3"].name, "A Faire");
});

Deno.test("tool handler — leantime_list_milestones enriches", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_list_milestones")!({ projectId: "3" }) as Record<string, unknown>;
  const parsed = JSON.parse((result.content as Array<Record<string, unknown>>)[0].text as string);
  assertEquals(Array.isArray(parsed), true);
  assertEquals(parsed.length, 2);
  assertEquals(parsed[0].statusLabel, "A Faire");
});

Deno.test("tool handler — leantime_list_sprints returns array", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_list_sprints")!({ projectId: "3" }) as Record<string, unknown>;
  const parsed = JSON.parse((result.content as Array<Record<string, unknown>>)[0].text as string);
  assertEquals(Array.isArray(parsed), true);
  assertEquals(parsed.length, 3);
});

Deno.test("tool handler — error wrapping on API failure", async () => {
  const { mockServer, tools } = createToolRegistry();
  const failingFetch = async () => new Response("Internal Server Error", { status: 500, statusText: "Internal Server Error" });
  const client = new LeantimeClient("https://leantime.test", "key", failingFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_list_projects")!({}) as Record<string, unknown>;
  const content = (result.content as Array<Record<string, unknown>>)[0];
  assertEquals(result.isError, true);
  assertEquals((content.text as string).startsWith("Error:"), true);
  assertEquals((content.text as string).includes("500"), true);
});

Deno.test("tool handler — error wrapping on RPC error", async () => {
  const { mockServer, tools } = createToolRegistry();
  const rpcErrorFetch = async () => new Response(JSON.stringify({
    jsonrpc: "2.0",
    error: { code: -32000, message: "Access denied" },
    id: 1,
  }), { status: 200, headers: { "Content-Type": "application/json" } });
  const client = new LeantimeClient("https://leantime.test", "key", rpcErrorFetch);
  registerAllTools(mockServer, client);

  const result = await tools.get("leantime_list_tickets")!({ projectId: "3" }) as Record<string, unknown>;
  assertEquals(result.isError, true);
  const content = (result.content as Array<Record<string, unknown>>)[0];
  assertEquals((content.text as string).includes("Access denied"), true);
});

Deno.test("tool handler — all 40 tools are registered", () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const expected = [
    "leantime_list_projects",
    "leantime_get_project",
    "leantime_get_project_progress",
    "leantime_create_project",
    "leantime_update_project",
    "leantime_find_projects",
    "leantime_list_project_users",
    "leantime_list_clients",
    "leantime_list_tickets",
    "leantime_get_ticket",
    "leantime_create_ticket",
    "leantime_update_ticket",
    "leantime_delete_ticket",
    "leantime_list_subtasks",
    "leantime_my_tasks",
    "leantime_get_ticket_options",
    "leantime_get_statuses",
    "leantime_get_ticket_types",
    "leantime_list_milestones",
    "leantime_get_milestone",
    "leantime_create_milestone",
    "leantime_update_milestone",
    "leantime_get_milestone_progress",
    "leantime_delete_milestone",
    "leantime_list_sprints",
    "leantime_create_sprint",
    "leantime_update_sprint",
    "leantime_get_current_sprint",
    "leantime_list_users",
    "leantime_list_comments",
    "leantime_add_comment",
    "leantime_update_comment",
    "leantime_delete_comment",
    "leantime_log_time",
    "leantime_get_ticket_time",
    "leantime_list_timesheets",
    "leantime_delete_timesheet_entry",
    "leantime_bulk_create_tickets",
    "leantime_bulk_update_tickets",
    "leantime_bulk_schedule_tickets",
  ];
  assertEquals([...tools.keys()].sort(), expected.sort());
  assertEquals(tools.size, 40);
});
