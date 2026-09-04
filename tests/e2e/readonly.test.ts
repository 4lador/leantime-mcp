import { assertEquals } from "@std/assert";
import { LeantimeClient } from "../../src/leantime-client.ts";
import type { LeantimeStatusMap } from "../../src/types.ts";
import { readKey, readUrl } from "../../src/keyring.ts";

// Environment first (per-run override, e.g. the local docker instance),
// then the keyring files (~/.config/leantime/).
const LEANTIME_URL = Deno.env.get("LEANTIME_URL") ?? await readUrl();
const LEANTIME_API_KEY = Deno.env.get("LEANTIME_API_KEY") ?? await readKey();
const hasCredentials = !!(LEANTIME_URL && LEANTIME_API_KEY);

Deno.test({
  name: "e2e — list projects",
  ignore: !hasCredentials,
  fn: async () => {
    const client = new LeantimeClient(LEANTIME_URL!, LEANTIME_API_KEY!);
    const result = await client.call<Record<string, unknown>[]>("Projects.getAll");
    assertEquals(Array.isArray(result), true);
    assertEquals(result.length > 0, true);
  },
});

Deno.test({
  name: "e2e — get statuses returns valid map",
  ignore: !hasCredentials,
  fn: async () => {
    const client = new LeantimeClient(LEANTIME_URL!, LEANTIME_API_KEY!);
    const projects = await client.call<Record<string, unknown>[]>("Projects.getAll");
    const projectId = String(projects[0].id);

    const statuses = await client.call<Record<string, Record<string, unknown>>>("tickets.getStatusLabels", { projectId });
    assertEquals(typeof statuses, "object");
    assertEquals(statuses !== null, true);

    for (const [_key, val] of Object.entries(statuses)) {
      assertEquals(typeof val.name, "string");
      assertEquals(typeof val.statusType, "string");
    }
  },
});

Deno.test({
  name: "e2e — list tickets with enrichment",
  ignore: !hasCredentials,
  fn: async () => {
    const client = new LeantimeClient(LEANTIME_URL!, LEANTIME_API_KEY!);
    const projects = await client.call<Record<string, unknown>[]>("Projects.getAll");
    const projectId = String(projects[0].id);

    const tickets = await client.call<Record<string, unknown>[]>("tickets.getAll", { currentProject: projectId });
    assertEquals(Array.isArray(tickets), true);

    if (tickets.length > 0) {
      const enriched = await client.enrichWithStatuses(tickets, projectId);
      const first = enriched[0] as Record<string, unknown>;
      assertEquals(typeof first.statusLabel, "string");
      assertEquals(typeof first.statusType, "string");
      assertEquals(typeof first.statusColor, "string");
    }
  },
});

Deno.test({
  name: "e2e — list milestones",
  ignore: !hasCredentials,
  fn: async () => {
    const client = new LeantimeClient(LEANTIME_URL!, LEANTIME_API_KEY!);
    const projects = await client.call<Record<string, unknown>[]>("Projects.getAll");
    const projectId = String(projects[0].id);

    const milestones = await client.call<Record<string, unknown>[]>("tickets.getAll", {
      currentProject: projectId,
      type: "milestone",
    });
    assertEquals(Array.isArray(milestones), true);
  },
});
