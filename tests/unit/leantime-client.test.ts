import { assertEquals, assertRejects } from "@std/assert";
import { LeantimeClient, enrichItem } from "../../src/leantime-client.ts";
import { createMockFetch } from "../helpers/mock-fetch.ts";
import { STATUS_MAP, TICKETS, RPC_OK, RPC_ERROR } from "../helpers/fixtures.ts";

Deno.test("enrichItem — adds statusLabel/statusType/statusColor", () => {
  const item = { id: "1", headline: "Test", status: 0 };
  const result = enrichItem(item, STATUS_MAP) as Record<string, unknown>;
  assertEquals(result.statusLabel, "Terminé");
  assertEquals(result.statusType, "DONE");
  assertEquals(result.statusColor, "label-success");
});

Deno.test("enrichItem — preserves original fields", () => {
  const item = { id: "1", headline: "Test", status: 3, projectId: "5" };
  const result = enrichItem(item, STATUS_MAP) as Record<string, unknown>;
  assertEquals(result.id, "1");
  assertEquals(result.headline, "Test");
  assertEquals(result.projectId, "5");
  assertEquals(result.statusLabel, "A Faire");
  assertEquals(result.statusType, "NEW");
});

Deno.test("enrichItem — returns unchanged for unknown status", () => {
  const item = { id: "1", headline: "Test", status: 99 };
  const result = enrichItem(item, STATUS_MAP);
  assertEquals(result, item);
  assertEquals((result as Record<string, unknown>).statusLabel, undefined);
});

Deno.test("enrichItem — returns unchanged for empty status map", () => {
  const item = { id: "1", headline: "Test", status: 0 };
  const result = enrichItem(item, {});
  assertEquals(result, item);
});

Deno.test("enrichItem — handles string status", () => {
  const item = { id: "1", headline: "Test", status: "3" };
  const result = enrichItem(item, STATUS_MAP) as Record<string, unknown>;
  assertEquals(result.statusLabel, "A Faire");
});

Deno.test("LeantimeClient.call — sends correct JSON-RPC request", async () => {
  const { fetch: mockFetch, calls } = createMockFetch({
    "leantime.rpc.test.method": (_method, params) => RPC_OK({ received: params }),
  });
  const client = new LeantimeClient("https://leantime.test", "my-api-key", mockFetch);
  await client.call("test.method", { foo: "bar" });
  assertEquals(calls.length, 1);
  assertEquals(calls[0].method, "leantime.rpc.test.method");
  assertEquals(calls[0].params, { foo: "bar" });
});

Deno.test("LeantimeClient.call — increments rpcId", async () => {
  const { fetch: mockFetch, calls } = createMockFetch({
    "leantime.rpc.method1": () => RPC_OK(null),
    "leantime.rpc.method2": () => RPC_OK(null),
  });
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  await client.call("method1");
  await client.call("method2");
  assertEquals(calls.length, 2);
});

Deno.test("LeantimeClient.call — throws on HTTP error", async () => {
  const mockFetch = async () => new Response("not found", { status: 404, statusText: "Not Found" });
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  await assertRejects(
    () => client.call("test"),
    Error,
    "Leantime API error: 404 Not Found",
  );
});

Deno.test("LeantimeClient.call — throws on RPC error", async () => {
  const mockFetch = async () => new Response(JSON.stringify(RPC_ERROR(-32000, "Something went wrong", "extra data")), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  await assertRejects(
    () => client.call("test"),
    Error,
    "Leantime RPC error [-32000]: Something went wrong — extra data",
  );
});

Deno.test("LeantimeClient.call — throws on RPC error without data", async () => {
  const mockFetch = async () => new Response(JSON.stringify(RPC_ERROR(-32000, "Fail")), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  await assertRejects(
    () => client.call("test"),
    Error,
    "Leantime RPC error [-32000]: Fail",
  );
});

Deno.test("LeantimeClient.enrichWithStatuses — enriches all items", async () => {
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  const result = await client.enrichWithStatuses(
    TICKETS.map((t) => ({ ...t })),
    "3",
  );
  const r = result as Record<string, unknown>[];
  assertEquals(r[0].statusLabel, "Terminé");
  assertEquals(r[0].statusType, "DONE");
  assertEquals(r[1].statusLabel, "En cours");
  assertEquals(r[1].statusType, "INPROGRESS");
  assertEquals(r[2].statusLabel, "A Faire");
  assertEquals(r[3].statusLabel, "Bloqué");
});

Deno.test("LeantimeClient.enrichWithStatuses — unknown status passes through", async () => {
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  const result = await client.enrichWithStatuses([{ id: "99", headline: "Weird", status: 99 }], "3");
  assertEquals(result[0].status, 99);
  assertEquals((result[0] as Record<string, unknown>).statusLabel, undefined);
});

Deno.test("LeantimeClient.enrichSingleWithStatuses — enriches a single item", async () => {
  const { fetch: mockFetch } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  const result = await client.enrichSingleWithStatuses({ id: "1", headline: "Test", status: 3 }, "3") as Record<string, unknown>;
  assertEquals(result.statusLabel, "A Faire");
  assertEquals(result.statusType, "NEW");
});

Deno.test("LeantimeClient — status cache avoids duplicate fetches", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  await client.enrichWithStatuses([{ id: "1", status: 0 }], "3");
  const callCountAfterFirst = calls.length;
  await client.enrichWithStatuses([{ id: "2", status: 3 }], "3");
  assertEquals(calls.length, callCountAfterFirst);
});

Deno.test("LeantimeClient — different projects have separate caches", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);
  await client.enrichWithStatuses([{ id: "1", status: 0 }], "3");
  await client.enrichWithStatuses([{ id: "2", status: 0 }], "4");
  const statusCalls = calls.filter((c) => c.method === "leantime.rpc.tickets.getStatusLabels");
  assertEquals(statusCalls.length, 2);
});

Deno.test("getUsers — caches users.getAll within TTL", async () => {
  const { fetch: mockFetch, calls } = createMockFetch();
  const client = new LeantimeClient("https://leantime.test", "key", mockFetch);

  const first = await client.getUsers();
  const second = await client.getUsers();

  assertEquals(first.length, 2);
  assertEquals(second.length, 2);
  assertEquals(calls.filter((c) => c.method === "leantime.rpc.users.getAll").length, 1);
});
