import { McpServer } from "@mcp/server";
import type { LeantimeClient } from "../leantime-client.ts";
import { registerProjectTools } from "./projects.ts";
import { registerTicketTools } from "./tickets.ts";
import { registerMilestoneTools } from "./milestones.ts";
import { registerUserTools } from "./users.ts";
import { registerCommentTools } from "./comments.ts";
import { registerTimesheetTools } from "./timesheets.ts";
import { registerSprintTools } from "./sprints.ts";
import { registerBulkTools } from "./bulk.ts";

export function registerAllTools(server: McpServer, client: LeantimeClient) {
  registerProjectTools(server, client);
  registerTicketTools(server, client);
  registerMilestoneTools(server, client);
  registerUserTools(server, client);
  registerCommentTools(server, client);
  registerTimesheetTools(server, client);
  registerSprintTools(server, client);
  registerBulkTools(server, client);
}
