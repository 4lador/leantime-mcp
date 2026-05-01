import { assertEquals } from "@std/assert";
import { LeantimeClient } from "../../src/leantime-client.ts";
import { createMockFetch } from "../helpers/mock-fetch.ts";

Deno.test("integration — list tickets with enrichment", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  const result = await client.call<Record<string, unknown>[]>("tickets.getAll", { currentProject: "3" });
  const enriched = await client.enrichWithStatuses(result, "3");

  assertEquals(enriched.length, 5);
  assertEquals((enriched[0] as Record<string, unknown>).statusLabel, "Terminé");
  assertEquals((enriched[0] as Record<string, unknown>).statusType, "DONE");
  assertEquals((enriched[1] as Record<string, unknown>).statusLabel, "En cours");
  assertEquals((enriched[2] as Record<string, unknown>).statusLabel, "A Faire");
});

Deno.test("integration — get single ticket with enrichment", async () => {
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  const result = await client.call<Record<string, unknown>>("tickets.getTicket", { id: "1", projectId: "3" });
  const enriched = (await client.enrichSingleWithStatuses(result, "3")) as Record<string, unknown>;

  assertEquals(enriched.statusLabel, "Terminé");
  assertEquals(enriched.statusType, "DONE");
  assertEquals(enriched.statusColor, "label-success");
});

Deno.test("integration — list milestones with enrichment", async () => {
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  const result = await client.call<Record<string, unknown>[]>("tickets.getAll", { currentProject: "3", type: "milestone" });
  const enriched = await client.enrichWithStatuses(result, "3");

  assertEquals(enriched.length, 2);
  assertEquals((enriched[0] as Record<string, unknown>).statusLabel, "A Faire");
  assertEquals((enriched[0] as Record<string, unknown>).statusType, "NEW");
});

Deno.test("integration — status cache: getStatusLabels called once for multiple enrichments", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  const tickets = await client.call<Record<string, unknown>[]>("tickets.getAll", { currentProject: "3" });
  await client.enrichWithStatuses(tickets, "3");

  const milestoneTickets = await client.call<Record<string, unknown>[]>("tickets.getAll", { currentProject: "3", type: "milestone" });
  await client.enrichWithStatuses(milestoneTickets, "3");

  const statusCalls = calls.filter((c) => c.method === "leantime.rpc.tickets.getStatusLabels");
  assertEquals(statusCalls.length, 1);
});

Deno.test("integration — create ticket sends correct payload", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.addTicket", {
    headline: "New task",
    projectId: "3",
    type: "task",
    milestoneid: "22",
    priority: 3,
  });

  const params = calls[0].params;
  assertEquals(params.headline, "New task");
  assertEquals(params.projectId, "3");
  assertEquals(params.type, "task");
  assertEquals(params.milestoneid, "22");
  assertEquals(params.priority, 3);
});

Deno.test("integration — list projects returns array", async () => {
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  const result = await client.call<Record<string, unknown>[]>("Projects.getAll");
  assertEquals(Array.isArray(result), true);
  assertEquals(result.length, 2);
  assertEquals(result[0].name, "Vision");
});

Deno.test("integration — list sprints returns array", async () => {
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  const result = await client.call<Record<string, unknown>[]>("sprints.getAllSprints", { projectId: "3" });
  assertEquals(Array.isArray(result), true);
  assertEquals(result.length, 1);
});

Deno.test("integration — get statuses returns map", async () => {
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  const result = await client.call<Record<string, Record<string, unknown>>>("tickets.getStatusLabels", { projectId: "3" });
  assertEquals(result["0"].name, "Terminé");
  assertEquals(result["0"].statusType, "DONE");
  assertEquals(result["3"].name, "A Faire");
});
