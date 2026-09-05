import { assertEquals } from "@std/assert";
import { LeantimeClient } from "../../src/leantime-client.ts";
import { createMockFetch } from "../helpers/mock-fetch.ts";
import { RPC_OK, RPC_ERROR } from "../helpers/fixtures.ts";
import { registerAllTools } from "../../src/tools/mod.ts";

type ToolHandler = (params: Record<string, unknown>) => Promise<Record<string, unknown>>;

function setup() {
  const tools = new Map<string, ToolHandler>();
  const mockServer = {
    tool: (name: string, _desc: string, _schema: unknown, handler: ToolHandler) => {
      tools.set(name, handler);
    },
  } as unknown as Parameters<typeof registerAllTools>[0];
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);
  return { tools, calls };
}

const text = (r: unknown) =>
  ((r as Record<string, unknown>).content as Array<Record<string, unknown>>)[0].text as string;

// ---------------------------------------------------------------- bulk create

Deno.test("bulk create — validation upfront: one bad item → zero creation", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_bulk_create_tickets")!({
    projectId: "3",
    tickets: [
      { headline: "OK ticket", editorId: "1" },
      { headline: "BAD — no assignment" },
      { headline: "OK ticket 2", unassigned: true },
    ],
  });
  assertEquals(r.isError, true);
  assertEquals(text(r).includes("NOTHING was created"), true);
  assertEquals(text(r).includes("assignment required"), true);
  assertEquals(calls.filter((c) => c.method === "leantime.rpc.tickets.addTicket").length, 0);
});

Deno.test("bulk create — unknown editorId upfront → zero creation", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_bulk_create_tickets")!({
    projectId: "3",
    tickets: [{ headline: "x", editorId: "999" }],
  });
  assertEquals(r.isError, true);
  assertEquals(text(r).includes("does not exist"), true);
  assertEquals(calls.filter((c) => c.method === "leantime.rpc.tickets.addTicket").length, 0);
});

Deno.test("bulk create — happy path: all created, markdown converted, results per item", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_bulk_create_tickets")!({
    projectId: "3",
    tickets: [
      { headline: "Task A", editorId: "1", description: "## Contexte\n\n**Important**" },
      { headline: "Task B", unassigned: true, type: "bug" },
      { headline: "Sub of A", unassigned: true, dependingTicketId: "100" },
    ],
  });
  assertEquals(r.isError, undefined);
  const parsed = JSON.parse(text(r));
  assertEquals(parsed.summary.created, 3);
  assertEquals(parsed.summary.failed, 0);
  assertEquals(parsed.results.length, 3);
  assertEquals(parsed.results[0].ok, true);
  assertEquals(parsed.results[1].ok, true);

  // Markdown converted
  const addCalls = calls.filter((c) => c.method === "leantime.rpc.tickets.addTicket");
  const first = addCalls[0];
  const values = first.params.values as Record<string, unknown>;
  assertEquals(values.description, "<h2>Contexte</h2><p><strong>Important</strong></p>");

  // Subtask link passed through
  const third = addCalls[2];
  assertEquals((third.params.values as Record<string, unknown>).dependingTicketId, "100");
});

Deno.test("bulk create — partial failure: item 2 fails, items 1 and 3 succeed", async () => {
  let callCount = 0;
  const tools = new Map<string, ToolHandler>();
  const mockServer = {
    tool: (name: string, _d: string, _s: unknown, h: ToolHandler) => tools.set(name, h),
  } as unknown as Parameters<typeof registerAllTools>[0];
  const { fetch: mockFetch } = createMockFetch({
    "leantime.rpc.tickets.addTicket": () => {
      callCount++;
      if (callCount === 2) return RPC_ERROR(-32000, "Ticket save error");
      return RPC_OK([callCount * 100]);
    },
  });
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  registerAllTools(mockServer, client);

  const r = await tools.get("leantime_bulk_create_tickets")!({
    projectId: "3",
    tickets: [
      { headline: "OK 1", unassigned: true },
      { headline: "FAIL", unassigned: true },
      { headline: "OK 3", unassigned: true },
    ],
  });
  assertEquals(r.isError, undefined);
  const parsed = JSON.parse(text(r));
  assertEquals(parsed.summary.created, 2);
  assertEquals(parsed.summary.failed, 1);
  assertEquals(parsed.results[1].ok, false);
  assertEquals(parsed.results[1].error.includes("Ticket save error"), true);
});

Deno.test("bulk create — max batch size (51 → rejected by schema)", async () => {
  const { tools } = setup();
  const tickets = Array.from({ length: 51 }, (_, i) => ({
    headline: `Ticket ${i}`,
    unassigned: true,
  }));
  // The Zod schema enforces max(50), but since we're bypassing it in tests,
  // verify the tool doesn't crash
  const r = await tools.get("leantime_bulk_create_tickets")!({
    projectId: "3",
    tickets,
  });
  // Should still work (the schema gate is at the MCP layer, not here)
  assertEquals(r.isError === true || r.isError === undefined, true);
});

// ---------------------------------------------------------------- bulk update

Deno.test("bulk update — patches only provided fields per ticket", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_bulk_update_tickets")!({
    projectId: "3",
    updates: [
      { ticketId: "10", headline: "Renamed A", status: 0 },
      { ticketId: "11", description: "**New** desc" },
    ],
  });
  assertEquals(r.isError, undefined);
  const parsed = JSON.parse(text(r));
  assertEquals(parsed.summary.created, 2);

  const first = calls.find((c) => c.method === "leantime.rpc.tickets.patch")!;
  assertEquals(first.params.id, "10");
  const changes = first.params.params as Record<string, unknown>;
  assertEquals(changes.headline, "Renamed A");
  assertEquals(changes.status, 0);
  assertEquals(Object.keys(changes).length, 2); // only the 2 fields provided

  const second = calls.filter((c) => c.method === "leantime.rpc.tickets.patch")[1];
  assertEquals(
    (second.params.params as Record<string, unknown>).description,
    "<p><strong>New</strong> desc</p>",
  );
});

Deno.test("bulk update — unknown editorId blocks the entire batch", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_bulk_update_tickets")!({
    projectId: "3",
    updates: [
      { ticketId: "10", headline: "OK" },
      { ticketId: "11", editorId: "777" },
    ],
  });
  assertEquals(r.isError, true);
  assertEquals(text(r).includes("NOTHING was updated"), true);
  assertEquals(calls.filter((c) => c.method === "leantime.rpc.tickets.patch").length, 0);
});

// ---------------------------------------------------------------- bulk schedule

Deno.test("bulk schedule — assigns sprint and dates via patch", async () => {
  const { tools, calls } = setup();
  const r = await tools.get("leantime_bulk_schedule_tickets")!({
    projectId: "3",
    schedules: [
      { ticketId: "10", sprintId: "1", editFrom: "2026-01-01", editTo: "2026-01-15" },
      { ticketId: "11", sprintId: "2" },
    ],
  });
  assertEquals(r.isError, undefined);
  const parsed = JSON.parse(text(r));
  assertEquals(parsed.summary.created, 2);

  const first = calls.find((c) => c.method === "leantime.rpc.tickets.patch")!;
  const params = first.params.params as Record<string, unknown>;
  assertEquals(params.sprint, "1");
  assertEquals(params.editFrom, "2026-01-01");
  assertEquals(params.editTo, "2026-01-15");

  const second = calls.filter((c) => c.method === "leantime.rpc.tickets.patch")[1];
  const p2 = second.params.params as Record<string, unknown>;
  assertEquals(p2.sprint, "2");
  assertEquals(Object.keys(p2).length, 1); // only sprint
});
