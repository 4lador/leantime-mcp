import { McpServer } from "@mcp/server";
import { z } from "zod";
import type { LeantimeClient } from "../leantime-client.ts";

function errorResult(message: string) {
  return {
    content: [{ type: "text" as const, text: `Error: ${message}` }],
    isError: true,
  };
}

export function registerTicketTools(server: McpServer, client: LeantimeClient) {
  server.tool(
    "leantime_list_tickets",
    "List tickets/tasks for a project with optional filters",
    {
      projectId: z.string().describe("The project ID to list tickets for"),
      status: z.string().optional().describe("Filter by status name or ID"),
      milestoneId: z.string().optional().describe("Filter by milestone ID"),
      sprintId: z.string().optional().describe("Filter by sprint ID"),
      userId: z.string().optional().describe("Filter by assigned user ID"),
      type: z.string().optional().describe("Filter by ticket type (task, story, bug, etc.)"),
      search: z.string().optional().describe("Search term for ticket headline/description"),
    },
    async (params) => {
      try {
        const searchParams: Record<string, unknown> = {
          currentProject: params.projectId,
        };
        if (params.status) searchParams.status = params.status;
        if (params.milestoneId) searchParams.milestoneId = params.milestoneId;
        if (params.sprintId) searchParams.sprint = params.sprintId;
        if (params.userId) searchParams.userId = params.userId;
        if (params.type) searchParams.type = params.type;
        if (params.search) searchParams.search = params.search;

        const result = await client.call<Record<string, unknown>[]>(
          "tickets.getAll",
          searchParams,
        );
        const enriched = await client.enrichWithStatuses(result, params.projectId);
        return {
          content: [{ type: "text", text: JSON.stringify(enriched, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_get_ticket",
    "Get details of a specific ticket/task",
    {
      projectId: z.string().describe("The project ID"),
      ticketId: z.string().describe("The ticket ID"),
    },
    async ({ projectId, ticketId }) => {
      try {
        const result = await client.call<Record<string, unknown>>(
          "tickets.getTicket",
          { id: ticketId, projectId },
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
    "leantime_create_ticket",
    "Create a new ticket/task in a project",
    {
      projectId: z.string().describe("The project ID"),
      headline: z.string().describe("Ticket title/headline"),
      description: z.string().optional().describe("Ticket description"),
      type: z.string().optional().describe("Ticket type (task, story, bug, etc.)"),
      priority: z.number().optional().describe("Priority (1-5)"),
      status: z.number().optional().describe("Status ID"),
      milestoneId: z.string().optional().describe("Milestone ID to assign to"),
      sprintId: z.string().optional().describe("Sprint ID to assign to"),
      userId: z.string().optional().describe("Assigned user ID"),
      tags: z.string().optional().describe("Comma-separated tags"),
      storypoints: z.string().optional().describe("Story points"),
      dateToFinish: z.string().optional().describe("Due date (YYYY-MM-DD)"),
    },
    async (params) => {
      try {
        const ticket: Record<string, unknown> = {
          headline: params.headline,
          projectId: params.projectId,
        };
        if (params.description) ticket.description = params.description;
        if (params.type) ticket.type = params.type;
        if (params.priority) ticket.priority = params.priority;
        if (params.status) ticket.status = params.status;
        if (params.milestoneId) ticket.milestoneid = params.milestoneId;
        if (params.sprintId) ticket.sprint = params.sprintId;
        if (params.userId) ticket.userId = params.userId;
        if (params.tags) ticket.tags = params.tags;
        if (params.storypoints) ticket.storypoints = params.storypoints;
        if (params.dateToFinish) ticket.dateToFinish = params.dateToFinish;

        const result = await client.call<unknown>(
          "tickets.addTicket",
          ticket,
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
    "leantime_update_ticket",
    "Update an existing ticket/task",
    {
      projectId: z.string().describe("The project ID"),
      ticketId: z.string().describe("The ticket ID"),
      headline: z.string().optional().describe("New ticket title"),
      description: z.string().optional().describe("New description"),
      type: z.string().optional().describe("New ticket type"),
      status: z.number().optional().describe("New status ID"),
      priority: z.number().optional().describe("New priority (1-5)"),
      milestoneId: z.string().optional().describe("New milestone ID"),
      sprintId: z.string().optional().describe("New sprint ID"),
      userId: z.string().optional().describe("New assigned user ID"),
      tags: z.string().optional().describe("New comma-separated tags"),
      storypoints: z.string().optional().describe("New story points"),
      dateToFinish: z.string().optional().describe("New due date (YYYY-MM-DD)"),
      percentDone: z.number().optional().describe("Percent done (0-100)"),
    },
    async ({ projectId, ticketId, ...updates }) => {
      try {
        const ticket: Record<string, unknown> = { id: ticketId, projectId };
        for (const [key, value] of Object.entries(updates)) {
          if (value !== undefined) {
            if (key === "milestoneId") ticket.milestoneid = value;
            else if (key === "sprintId") ticket.sprint = value;
            else if (key === "percentDone") ticket.percentDone = value;
            else ticket[key] = value;
          }
        }

        const result = await client.call<unknown>(
          "tickets.updateTicket",
          ticket,
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
    "leantime_get_statuses",
    "Get available status labels for a project",
    {
      projectId: z.string().describe("The project ID"),
    },
    async ({ projectId }) => {
      try {
        const result = await client.call<Record<string, unknown>>(
          "tickets.getStatusLabels",
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

  server.tool(
    "leantime_get_ticket_types",
    "Get available ticket types",
    {
      projectId: z.string().describe("The project ID"),
    },
    async ({ projectId }) => {
      try {
        const result = await client.call<Record<string, unknown>>(
          "tickets.getTicketTypes",
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
