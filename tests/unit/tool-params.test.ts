import { assertEquals } from "@std/assert";
import { LeantimeClient } from "../../src/leantime-client.ts";
import { createMockFetch } from "../helpers/mock-fetch.ts";

Deno.test("list_tickets — passes projectId as currentProject", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.getAll", { currentProject: "3" });
  assertEquals(calls[0].params.currentProject, "3");
});

Deno.test("list_tickets — optional filters map correctly", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.getAll", {
    currentProject: "3",
    status: "3",
    milestoneId: "22",
    sprint: "1",
    userId: "2",
    type: "task",
    search: "test",
  });

  const params = calls[0].params;
  assertEquals(params.currentProject, "3");
  assertEquals(params.status, "3");
  assertEquals(params.milestoneId, "22");
  assertEquals(params.sprint, "1");
  assertEquals(params.userId, "2");
  assertEquals(params.type, "task");
  assertEquals(params.search, "test");
});

Deno.test("list_tickets — omitted filters are not sent", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.getAll", { currentProject: "3" });

  const params = calls[0].params;
  assertEquals(Object.keys(params).length, 1);
  assertEquals(params.currentProject, "3");
});

Deno.test("create_ticket — maps camelCase to Leantime field names", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  const ticket: Record<string, unknown> = {
    headline: "Test",
    projectId: "3",
  };
  ticket.milestoneid = "22";
  ticket.sprint = "1";

  await client.call("tickets.addTicket", ticket);

  const params = calls[0].params;
  assertEquals(params.headline, "Test");
  assertEquals(params.projectId, "3");
  assertEquals(params.milestoneid, "22");
  assertEquals(params.sprint, "1");
});

Deno.test("update_ticket — maps camelCase fields", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  const ticket: Record<string, unknown> = { id: "42", projectId: "3" };
  ticket.milestoneid = "22";
  ticket.sprint = "1";
  ticket.percentDone = 50;

  await client.call("tickets.updateTicket", ticket);

  const params = calls[0].params;
  assertEquals(params.id, "42");
  assertEquals(params.milestoneid, "22");
  assertEquals(params.sprint, "1");
  assertEquals(params.percentDone, 50);
});

Deno.test("update_ticket — undefined values are excluded", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  const ticket: Record<string, unknown> = { id: "42", projectId: "3" };

  await client.call("tickets.updateTicket", ticket);

  const params = calls[0].params;
  assertEquals(Object.keys(params).length, 2);
  assertEquals(params.id, "42");
  assertEquals(params.projectId, "3");
});

Deno.test("get_ticket — passes id and projectId", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.getTicket", { id: "42", projectId: "3" });

  assertEquals(calls[0].params.id, "42");
  assertEquals(calls[0].params.projectId, "3");
});

Deno.test("list_milestones — passes type milestone", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.getAll", { currentProject: "3", type: "milestone" });

  assertEquals(calls[0].params.type, "milestone");
});

Deno.test("list_sprints — passes projectId", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("sprints.getAllSprints", { projectId: "3" });

  assertEquals(calls[0].params.projectId, "3");
});
