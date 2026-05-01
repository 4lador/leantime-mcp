import { McpServer } from "@mcp/server";
import { z } from "zod";
import type { LeantimeClient } from "../leantime-client.ts";

function errorResult(message: string) {
  return {
    content: [{ type: "text" as const, text: `Error: ${message}` }],
    isError: true,
  };
}

export function registerProjectTools(server: McpServer, client: LeantimeClient) {
  server.tool(
    "leantime_list_projects",
    "List all projects assigned to the current user",
    {},
    async () => {
      try {
        const result = await client.call<Record<string, unknown>[]>(
          "Projects.getAll",
        );
        return {
          content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_get_project",
    "Get details of a specific project",
    {
      projectId: z.string().describe("The project ID"),
    },
    async ({ projectId }) => {
      try {
        const result = await client.call<Record<string, unknown>>(
          "projects.getProject",
          { id: projectId },
        );
        return {
          content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_get_project_progress",
    "Get progress metrics for a specific project",
    {
      projectId: z.string().describe("The project ID"),
    },
    async ({ projectId }) => {
      try {
        const result = await client.call<Record<string, unknown>>(
          "projects.getProjectProgress",
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
