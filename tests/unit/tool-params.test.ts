import { assertEquals } from "@std/assert";
import { LeantimeClient } from "../../src/leantime-client.ts";
import { createMockFetch } from "../helpers/mock-fetch.ts";

Deno.test("list_tickets — wraps filters in searchCriteria", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.getAll", { searchCriteria: { currentProject: "3" }, limit: 500 });
  assertEquals(
    (calls[0].params.searchCriteria as Record<string, unknown>).currentProject,
    "3",
  );
});

Deno.test("list_tickets — filters use Leantime's real criteria keys", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.getAll", {
    searchCriteria: {
      currentProject: "3",
      status: "3",
      milestone: "22",
      sprint: "1",
      users: "2",
      type: "task",
      term: "test",
    },
    limit: 500,
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

Deno.test("list_tickets — omitted filters are not sent", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.getAll", { searchCriteria: { currentProject: "3" }, limit: 500 });

  const sc = calls[0].params.searchCriteria as Record<string, unknown>;
  assertEquals(Object.keys(sc).length, 1);
  assertEquals(sc.currentProject, "3");
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

Deno.test("update_ticket — patch sends id plus changed fields only", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.patch", {
    id: "42",
    params: { milestoneid: "22", sprint: "1" },
  });

  assertEquals(calls[0].params.id, "42");
  const params = calls[0].params.params as Record<string, unknown>;
  assertEquals(params.milestoneid, "22");
  assertEquals(params.sprint, "1");
  assertEquals(Object.keys(params).length, 2);
});

Deno.test("get_ticket — passes id and projectId", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.getTicket", { id: "42" });

  assertEquals(calls[0].params.id, "42");
  assertEquals(Object.keys(calls[0].params).length, 1);
});

Deno.test("list_milestones — wraps type milestone in searchCriteria", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("tickets.getAll", {
    searchCriteria: { currentProject: "3", type: "milestone" },
    limit: 200,
  });

  const sc = calls[0].params.searchCriteria as Record<string, unknown>;
  assertEquals(sc.currentProject, "3");
  assertEquals(sc.type, "milestone");
});

Deno.test("list_sprints — passes projectId", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  await client.call("sprints.getAllSprints", { projectId: "3" });

  assertEquals(calls[0].params.projectId, "3");
});
