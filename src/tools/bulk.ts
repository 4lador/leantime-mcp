import { McpServer } from "@mcp/server";
import { z } from "zod";
import type { LeantimeClient } from "../leantime-client.ts";
import { markdownToHtml } from "../markdown.ts";
import { errorResult, fetchUsers, usersList } from "./shared.ts";

const MAX_BATCH = 50;

const RATE_LIMIT_NOTE =
  "The server automatically retries on 429 rate limits (3 retries with " +
  "backoff) — you never need to handle rate limiting yourself. If a rate " +
  "limit error still surfaces, wait ~60 seconds before the next batch.";

interface BulkResult {
  index: number;
  ok: boolean;
  id?: string;
  error?: string;
}

function bulkSummary(results: BulkResult[]) {
  return {
    summary: {
      total: results.length,
      created: results.filter((r) => r.ok).length,
      failed: results.filter((r) => !r.ok).length,
    },
    results,
  };
}

const ticketSpecSchema = z.object({
  headline: z.string().describe("Ticket title"),
  description: z.string().optional().describe("Ticket description in Markdown"),
  editorId: z.string().optional().describe("Assigned user ID (required unless unassigned: true)"),
  unassigned: z.boolean().optional().describe("Set to true ONLY if explicitly requested"),
  type: z.string().optional().describe("Ticket type (task, story, bug, etc.)"),
  priority: z.number().optional().describe("Priority (1-5)"),
  milestoneId: z.string().optional().describe("Milestone ID"),
  sprintId: z.string().optional().describe("Sprint ID"),
  dependingTicketId: z.string().optional().describe("Parent ticket ID (for subtasks)"),
  tags: z.string().optional().describe("Comma-separated tags"),
  dateToFinish: z.string().optional().describe("Due date (YYYY-MM-DD)"),
  planHours: z.number().optional().describe("Planned hours"),
});

const updateSpecSchema = z.object({
  ticketId: z.string().describe("The ticket ID to update"),
  headline: z.string().optional().describe("New title"),
  description: z.string().optional().describe("New description in Markdown"),
  type: z.string().optional().describe("New type"),
  status: z.number().optional().describe("New status ID"),
  priority: z.number().optional().describe("New priority (1-5)"),
  milestoneId: z.string().optional().describe("New milestone ID"),
  sprintId: z.string().optional().describe("New sprint ID"),
  editorId: z.string().optional().describe("New assigned user ID"),
  tags: z.string().optional().describe("New comma-separated tags"),
  storypoints: z.string().optional().describe("New story points"),
  dateToFinish: z.string().optional().describe("New due date (YYYY-MM-DD)"),
  planHours: z.number().optional().describe("New planned hours"),
});

const scheduleSpecSchema = z.object({
  ticketId: z.string().describe("The ticket ID to schedule"),
  sprintId: z.string().optional().describe("Sprint ID to assign to"),
  editFrom: z.string().optional().describe("Scheduled start date (YYYY-MM-DD)"),
  editTo: z.string().optional().describe("Scheduled end date (YYYY-MM-DD)"),
});

export function registerBulkTools(server: McpServer, client: LeantimeClient) {
  server.tool(
    "leantime_bulk_create_tickets",
    `Create multiple tickets in one call (max ${MAX_BATCH}). ALL items are validated ` +
      `BEFORE anything is created — if any item fails validation (missing assignment, ` +
      `unknown editorId), nothing is created. Descriptions are Markdown, converted to ` +
      `rich HTML per ticket. Each item requires editorId or unassigned: true. ${RATE_LIMIT_NOTE}`,
    {
      projectId: z.string().describe("The project ID"),
      tickets: z.array(ticketSpecSchema).min(1).max(MAX_BATCH).describe(
        `Array of ticket specifications (max ${MAX_BATCH})`,
      ),
    },
    async (args: { projectId: string; tickets: unknown[] }) => {
      try {
        const { projectId, tickets } = args;
        // Leantime's addTicket expects an int projectId — sending a string
        // works for individual tools but causes a TypeError inside the bulk
        // path (isUserAssignedToProject receives null). Coerce to number.
        const pidNum = Number(projectId);
        // ---- Phase 1: Upfront validation (zero API writes) ----
        const users = await fetchUsers(client);
        const validationErrors: string[] = [];

        for (let i = 0; i < tickets.length; i++) {
          const t = tickets[i] as Record<string, unknown>;
          const idx = i + 1;
          if (!t.editorId && !t.unassigned) {
            validationErrors.push(
              `Item ${idx} ("${t.headline}"): assignment required — pass editorId or unassigned: true`,
            );
          }
          if (t.editorId && !users.some((u) => u.id === String(t.editorId))) {
            validationErrors.push(
              `Item ${idx}: editorId "${t.editorId}" does not exist. Available: ${usersList(users)}`,
            );
          }
        }

        if (validationErrors.length > 0) {
          return errorResult(
            `Validation failed — NOTHING was created (all-or-nothing):\n` +
              validationErrors.join("\n"),
          );
        }

        // ---- Phase 2: Sequential creation ----
        // Rate limiting is handled by LeantimeClient.call() which retries
        // on 429 with header-aware backoff — no fixed delay needed here.
        const results: BulkResult[] = [];
        for (let i = 0; i < tickets.length; i++) {
          const spec = tickets[i] as Record<string, unknown>;
          try {
            const values: Record<string, unknown> = {
              headline: spec.headline,
              projectId: pidNum,
            };
            if (spec.description) values.description = markdownToHtml(spec.description as string);
            if (spec.type) values.type = spec.type;
            if (spec.priority) values.priority = spec.priority;
            if (spec.milestoneId) values.milestoneid = spec.milestoneId;
            if (spec.sprintId) values.sprint = spec.sprintId;
            if (spec.editorId) values.editorId = spec.editorId;
            if (spec.tags) values.tags = spec.tags;
            if (spec.dateToFinish) values.dateToFinish = spec.dateToFinish;
            if (spec.dependingTicketId) values.dependingTicketId = spec.dependingTicketId;
            if (spec.planHours) values.planHours = spec.planHours;

            const result = await client.call<unknown>("tickets.addTicket", { values });
            if (Array.isArray(result) && result.length > 0) {
              results.push({ index: i + 1, ok: true, id: String(result[0]) });
            } else {
              results.push({ index: i + 1, ok: false, error: "unexpected API response" });
            }
          } catch (e) {
            results.push({
              index: i + 1,
              ok: false,
              error: e instanceof Error ? e.message : String(e),
            });
          }
        }

        return {
          content: [{
            type: "text",
            text: JSON.stringify(bulkSummary(results), null, 2),
          }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_bulk_update_tickets",
    `Update multiple tickets in one call (max ${MAX_BATCH}). Uses the safe patch ` +
      `API — only provided fields change, others are never wiped. Results are ` +
      `per-item: some may succeed while others fail. ${RATE_LIMIT_NOTE}`,
    {
      projectId: z.string().describe("The project ID"),
      updates: z.array(updateSpecSchema).min(1).max(MAX_BATCH).describe(
        `Array of ticket updates (max ${MAX_BATCH})`,
      ),
    },
    async ({ projectId, updates }) => {
      try {
        // Upfront: validate any editorId changes
        const editorIds = updates
          .map((u) => (u as Record<string, unknown>).editorId)
          .filter((id) => id !== undefined) as string[];
        if (editorIds.length > 0) {
          const users = await fetchUsers(client);
          for (const id of editorIds) {
            if (!users.some((u) => u.id === String(id))) {
              return errorResult(
                `editorId "${id}" does not exist — NOTHING was updated. ` +
                  `Available: ${usersList(users)}`,
              );
            }
          }
        }

        const results: BulkResult[] = [];
        for (let i = 0; i < updates.length; i++) {
          const spec = updates[i] as Record<string, unknown>;
          const ticketId = String(spec.ticketId);
          try {
            const changes: Record<string, unknown> = {};
            if (spec.headline !== undefined) changes.headline = spec.headline;
            if (spec.description !== undefined) {
              changes.description = markdownToHtml(spec.description as string);
            }
            if (spec.type !== undefined) changes.type = spec.type;
            if (spec.status !== undefined) changes.status = spec.status;
            if (spec.priority !== undefined) changes.priority = spec.priority;
            if (spec.milestoneId !== undefined) changes.milestoneid = spec.milestoneId;
            if (spec.sprintId !== undefined) changes.sprint = spec.sprintId;
            if (spec.editorId !== undefined) changes.editorId = spec.editorId;
            if (spec.tags !== undefined) changes.tags = spec.tags;
            if (spec.storypoints !== undefined) changes.storypoints = spec.storypoints;
            if (spec.dateToFinish !== undefined) changes.dateToFinish = spec.dateToFinish;
            if (spec.planHours !== undefined) changes.planHours = spec.planHours;

            if (Object.keys(changes).length === 0) {
              results.push({ index: i + 1, ok: false, error: "no fields to update" });
              continue;
            }

            const result = await client.call<boolean>("tickets.patch", {
              id: ticketId,
              params: changes,
            });
            // Leantime wraps some return values in arrays — handle both
            const patchOk = result === true || (Array.isArray(result) && result[0] === true);
            results.push({ index: i + 1, ok: patchOk, id: ticketId });
          } catch (e) {
            results.push({
              index: i + 1,
              ok: false,
              id: ticketId,
              error: e instanceof Error ? e.message : String(e),
            });
          }
        }

        return {
          content: [{
            type: "text",
            text: JSON.stringify(bulkSummary(results), null, 2),
          }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_bulk_schedule_tickets",
    `Schedule multiple tickets at once (max ${MAX_BATCH}): assign to a sprint and/or ` +
      `set editFrom/editTo dates. Uses the safe patch API. ${RATE_LIMIT_NOTE}`,
    {
      projectId: z.string().describe("The project ID"),
      schedules: z.array(scheduleSpecSchema).min(1).max(MAX_BATCH).describe(
        `Array of ticket schedules (max ${MAX_BATCH})`,
      ),
    },
    async ({ schedules }) => {
      try {
        const results: BulkResult[] = [];
        for (let i = 0; i < schedules.length; i++) {
          const spec = schedules[i] as Record<string, unknown>;
          const ticketId = String(spec.ticketId);
          try {
            const changes: Record<string, unknown> = {};
            if (spec.sprintId !== undefined) changes.sprint = spec.sprintId;
            if (spec.editFrom !== undefined) changes.editFrom = spec.editFrom;
            if (spec.editTo !== undefined) changes.editTo = spec.editTo;

            if (Object.keys(changes).length === 0) {
              results.push({ index: i + 1, ok: false, error: "nothing to schedule" });
              continue;
            }

            const result = await client.call<boolean>("tickets.patch", {
              id: ticketId,
              params: changes,
            });
            // Leantime wraps some return values in arrays — handle both
            const patchOk = result === true || (Array.isArray(result) && result[0] === true);
            results.push({ index: i + 1, ok: patchOk, id: ticketId });
          } catch (e) {
            results.push({
              index: i + 1,
              ok: false,
              id: ticketId,
              error: e instanceof Error ? e.message : String(e),
            });
          }
        }

        return {
          content: [{
            type: "text",
            text: JSON.stringify(bulkSummary(results), null, 2),
          }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );
}
