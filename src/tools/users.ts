import { McpServer } from "@mcp/server";
import type { LeantimeClient } from "../leantime-client.ts";

function errorResult(message: string) {
  return {
    content: [{ type: "text" as const, text: `Error: ${message}` }],
    isError: true,
  };
}

export function registerUserTools(server: McpServer, client: LeantimeClient) {
  server.tool(
    "leantime_list_users",
    "List all users (id and name). Use this to pick a valid editorId when assigning tickets.",
    {},
    async () => {
      try {
        const users = await client.getUsers();
        const simplified = users.map((u) => ({
          id: String(u.id),
          name: [u.firstname, u.lastname].filter(Boolean).join(" ").trim(),
        }));
        return {
          content: [{ type: "text", text: JSON.stringify(simplified, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );
}
