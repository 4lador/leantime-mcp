import { McpServer } from "@mcp/server";
import { z } from "zod";
import type { LeantimeClient } from "../leantime-client.ts";

function errorResult(message: string) {
  return {
    content: [{ type: "text" as const, text: `Error: ${message}` }],
    isError: true,
  };
}

export function registerMilestoneTools(
  server: McpServer,
  client: LeantimeClient,
) {
  server.tool(
    "leantime_list_milestones",
    "List all milestones for a project",
    {
      projectId: z.string().describe("The project ID"),
    },
    async ({ projectId }) => {
      try {
        const result = await client.call<Record<string, unknown>[]>(
          "tickets.getAll",
          {
            searchCriteria: { currentProject: projectId, type: "milestone" },
            limit: 200,
          },
        );
        const enriched = await client.enrichWithStatuses(result, projectId);
        return {
          content: [{ type: "text", text: JSON.stringify(enriched, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_get_milestone",
    "Get details of a specific milestone",
    {
      projectId: z.string().describe("The project ID"),
      milestoneId: z.string().describe("The milestone ID"),
    },
    async ({ projectId, milestoneId }) => {
      try {
        const result = await client.call<Record<string, unknown>>(
          "tickets.getTicket",
          { id: milestoneId },
        );
        const enriched = await client.enrichSingleWithStatuses(result, projectId);
        return {
          content: [{ type: "text", text: JSON.stringify(enriched, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

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
          content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );
}
