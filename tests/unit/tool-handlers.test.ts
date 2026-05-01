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
  assertEquals(parsed.length, 5);
  assertEquals(parsed[0].statusLabel, "Terminé");
  assertEquals(parsed[0].statusType, "DONE");
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
  }) as Record<string, unknown>;
  const content = (result.content as Array<Record<string, unknown>>)[0];
  assertEquals(content.type, "text");
  assertEquals(calls[0].method, "leantime.rpc.tickets.addTicket");
  assertEquals(calls[0].params.headline, "New task");
});

Deno.test("tool handler — leantime_update_ticket maps fields correctly", async () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  await tools.get("leantime_update_ticket")!({
    projectId: "3",
    ticketId: "42",
    milestoneId: "22",
    percentDone: 75,
  });
  const params = calls[0].params;
  assertEquals(params.id, "42");
  assertEquals(params.milestoneid, "22");
  assertEquals(params.percentDone, 75);
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
  assertEquals(parsed.length, 1);
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

Deno.test("tool handler — all 12 tools are registered", () => {
  const { mockServer, tools } = createToolRegistry();
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const expected = [
    "leantime_list_projects",
    "leantime_get_project",
    "leantime_get_project_progress",
    "leantime_list_tickets",
    "leantime_get_ticket",
    "leantime_create_ticket",
    "leantime_update_ticket",
    "leantime_get_statuses",
    "leantime_get_ticket_types",
    "leantime_list_milestones",
    "leantime_get_milestone",
    "leantime_list_sprints",
  ];
  assertEquals([...tools.keys()].sort(), expected.sort());
  assertEquals(tools.size, 12);
});
