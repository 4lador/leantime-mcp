import { assertEquals } from "@std/assert";
import { LeantimeClient } from "../../src/leantime-client.ts";
import { readKey, readUrl } from "../../src/keyring.ts";

// Environment first (per-run override, e.g. the local docker instance),
// then the keyring files (~/.config/leantime/).
const LEANTIME_URL = Deno.env.get("LEANTIME_URL") ?? await readUrl();
const LEANTIME_API_KEY = Deno.env.get("LEANTIME_API_KEY") ?? await readKey();
const hasCredentials = !!(LEANTIME_URL && LEANTIME_API_KEY);

/**
 * Loud skip: an empty instance is a legitimate state, but a silent pass on
 * empty data is a vacuous test — it once masked a total data loss ("e2e 4/4"
 * while the instance was empty). Always say what was skipped.
 */
function loudSkip(reason: string): void {
  console.warn(`  ⚠ VACUOUS-SKIP: ${reason}`);
}

Deno.test({
  name: "e2e — list projects",
  ignore: !hasCredentials,
  fn: async () => {
    const client = new LeantimeClient(LEANTIME_URL!, LEANTIME_API_KEY!);
    const result = await client.call<Record<string, unknown>[]>("Projects.getAll");
    assertEquals(Array.isArray(result), true);
    // A configured key assigned to zero projects sees nothing — that is a
    // configuration problem worth failing loudly about, not skipping.
    assertEquals(
      result.length > 0,
      true,
      "API key is assigned to no project — fix with `leantmcp key rotate` or assign it in the Leantime UI",
    );
  },
});

Deno.test({
  name: "e2e — get statuses returns a non-empty map",
  ignore: !hasCredentials,
  fn: async () => {
    const client = new LeantimeClient(LEANTIME_URL!, LEANTIME_API_KEY!);
    const projects = await client.call<Record<string, unknown>[]>("Projects.getAll");
    const projectId = String(projects[0].id);

    const statuses = await client.call<Record<string, Record<string, unknown>>>(
      "tickets.getStatusLabels",
      { projectId },
    );
    assertEquals(typeof statuses, "object");
    assertEquals(statuses !== null, true);
    // Non-vacuous: any real project has at least one status label.
    assertEquals(
      Object.keys(statuses).length > 0,
      true,
      `project ${projectId} has no status labels`,
    );

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

    const tickets = await client.call<Record<string, unknown>[]>("tickets.getAll", {
      searchCriteria: { currentProject: projectId },
      limit: 100,
    });
    assertEquals(Array.isArray(tickets), true);

    if (tickets.length === 0) {
      loudSkip(
        `project ${projectId} has 0 tickets — enrichment assertions NOT run ` +
          "(empty data would make them vacuous). Run the exhaustive local e2e " +
          "(LEANTIME_E2E=local) for full coverage.",
      );
      return;
    }

    const enriched = await client.enrichWithStatuses(tickets, projectId);
    const first = enriched[0] as Record<string, unknown>;
    assertEquals(typeof first.statusLabel, "string");
    assertEquals(typeof first.statusType, "string");
    assertEquals(typeof first.statusColor, "string");
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
      searchCriteria: { currentProject: projectId, type: "milestone" },
      limit: 100,
    });
    assertEquals(Array.isArray(milestones), true);

    if (milestones.length === 0) {
      loudSkip(
        `project ${projectId} has 0 milestones — array shape checked only. ` +
          "Run the exhaustive local e2e (LEANTIME_E2E=local) for full coverage.",
      );
      return;
    }

    assertEquals(
      milestones.every((m) => m.type === "milestone"),
      true,
    );
  },
});
