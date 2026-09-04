import { McpServer } from "@mcp/server";
import type { LeantimeClient } from "../leantime-client.ts";
import { registerProjectTools } from "./projects.ts";
import { registerTicketTools } from "./tickets.ts";
import { registerMilestoneTools } from "./milestones.ts";
import { registerUserTools } from "./users.ts";

export function registerAllTools(server: McpServer, client: LeantimeClient) {
  registerProjectTools(server, client);
  registerTicketTools(server, client);
  registerMilestoneTools(server, client);
  registerUserTools(server, client);
}
