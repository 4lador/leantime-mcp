import { McpServer } from "@mcp/server";
import { z } from "zod";
import type { LeantimeClient } from "../leantime-client.ts";
import { errorResult, isLeantimeError } from "./shared.ts";

export function registerSprintTools(server: McpServer, client: LeantimeClient) {
  server.tool(
    "leantime_list_sprints",
    "List all sprints for a project",
    {
      projectId: z.string().describe("The project ID"),
    },
    async ({ projectId }) => {
      try {
        const result = await client.call<Record<string, unknown>[]>(
          "sprints.getAllSprints",
          { projectId },
        );
        return {
          content: [{ type: "text", text: JSON.stringify(result ?? [], null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_create_sprint",
    "Create a sprint in a project",
    {
      projectId: z.string().describe("The project ID"),
      name: z.string().describe("Sprint name"),
      startDate: z.string().describe("Start date, YYYY-MM-DD"),
      endDate: z.string().describe("End date, YYYY-MM-DD"),
    },
    async ({ projectId, name, startDate, endDate }) => {
      try {
        // addSprint defaults projectId to the session's current project, which is
        // NOT set for API keys — always pass it explicitly.
        const result = await client.call<unknown>("sprints.addSprint", {
          params: { name, startDate, endDate, projectId },
        });
        if (result === false || result === null || isLeantimeError(result)) {
          return errorResult(`Sprint creation failed for project ${projectId}.`);
        }
        // addSprint may return the id wrapped in an array — normalize.
        const id = Array.isArray(result) ? result[0] : result;
        return {
          content: [{ type: "text", text: JSON.stringify({ id, projectId }, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_update_sprint",
    "Update a sprint (name and/or dates)",
    {
      sprintId: z.string().describe("The sprint ID"),
      name: z.string().optional().describe("New sprint name"),
      startDate: z.string().optional().describe("New start date, YYYY-MM-DD"),
      endDate: z.string().optional().describe("New end date, YYYY-MM-DD"),
    },
    async ({ sprintId, name, startDate, endDate }) => {
      try {
        if (name === undefined && startDate === undefined && endDate === undefined) {
          return errorResult("Nothing to update: provide name, startDate and/or endDate.");
        }
        // editSprint overwrites projectId from the session (unset for API keys):
        // fetch the sprint first and always resend its full field set.
        const sprint = await client.call<Record<string, unknown>>("sprints.getSprint", {
          id: sprintId,
        });
        if (typeof sprint === "boolean" || isLeantimeError(sprint)) {
          return errorResult(`Sprint ${sprintId} not found.`);
        }

        const result = await client.call<unknown>("sprints.editSprint", {
          params: {
            id: sprintId,
            projectId: sprint.projectId,
            name: name ?? sprint.name,
            startDate: startDate ?? sprint.startDate,
            endDate: endDate ?? sprint.endDate,
          },
        });
        return {
          content: [{
            type: "text",
            text: JSON.stringify({ ok: !isLeantimeError(result), sprintId }, null, 2),
          }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_get_current_sprint",
    "Get the sprint currently in progress for a project (falls back to the next upcoming one). " +
      "Computed from sprint dates — Leantime's session-based currentSprint is unavailable to API keys.",
    {
      projectId: z.string().describe("The project ID"),
    },
    async ({ projectId }) => {
      try {
        const sprints = await client.call<
          { id: unknown; name: unknown; startDate: string; endDate: string }[]
        >("sprints.getAllSprints", { projectId });
        const now = Date.now();
        const withTime = (sprints ?? []).map((s) => ({
          ...s,
          start: s.startDate ? new Date(s.startDate.replace(" ", "T")).getTime() : NaN,
          end: s.endDate ? new Date(s.endDate.replace(" ", "T")).getTime() : NaN,
        }));
        const current = withTime.find((s) => s.start <= now && now <= s.end) ?? null;
        const upcoming = withTime
          .filter((s) => s.start > now)
          .sort((a, b) => a.start - b.start)[0] ?? null;
        return {
          content: [{
            type: "text",
            text: JSON.stringify(
              {
                current,
                upcoming: current ? null : upcoming,
              },
              null,
              2,
            ),
          }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );
}
