import { McpServer } from "@mcp/server";
import { z } from "zod";
import type { LeantimeClient } from "../leantime-client.ts";
import { markdownToHtml } from "../markdown.ts";

function errorResult(message: string) {
  return {
    content: [{ type: "text" as const, text: `Error: ${message}` }],
    isError: true,
  };
}

const MARKDOWN_HINT =
  "Markdown (## headings, lists, - [ ] checklists, **bold**, `code`, links) — converted to rich HTML for Leantime's editor";

const ASSIGNMENT_RULE =
  "You MUST ask the user who the ticket should be assigned to before calling this tool.";

interface LeantimeUser {
  id: string;
  name: string;
}

async function fetchUsers(client: LeantimeClient): Promise<LeantimeUser[]> {
  const users = await client.getUsers();
  return users.map((u) => ({
    id: String(u.id),
    name: [u.firstname, u.lastname].filter(Boolean).join(" ").trim(),
  }));
}

function usersList(users: LeantimeUser[]): string {
  return users.map((u) => `${u.id} (${u.name})`).join(", ");
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
        // Leantime expects filters inside a `searchCriteria` object — flat
        // params are silently ignored and would return every ticket.
        const searchCriteria: Record<string, unknown> = {
          currentProject: params.projectId,
        };
        if (params.status) searchCriteria.status = params.status;
        if (params.milestoneId) searchCriteria.milestone = params.milestoneId;
        if (params.sprintId) searchCriteria.sprint = params.sprintId;
        if (params.userId) searchCriteria.users = params.userId;
        if (params.type) searchCriteria.type = params.type;
        if (params.search) searchCriteria.term = params.search;

        const result = await client.call<Record<string, unknown>[]>(
          "tickets.getAll",
          { searchCriteria, limit: 500 },
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
          { id: ticketId },
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
    `Create a new ticket/task in a project. The description is ${MARKDOWN_HINT}. ` +
      `${ASSIGNMENT_RULE} Pass the chosen editorId (get candidates with leantime_list_users), ` +
      `or pass unassigned: true ONLY if the user explicitly said to leave it unassigned.`,
    {
      projectId: z.string().describe("The project ID"),
      headline: z.string().describe("Ticket title/headline"),
      description: z.string().optional().describe(`Ticket description in ${MARKDOWN_HINT}`),
      type: z.string().optional().describe("Ticket type (task, story, bug, etc.)"),
      priority: z.number().optional().describe("Priority (1-5)"),
      status: z.number().optional().describe("Status ID"),
      milestoneId: z.string().optional().describe("Milestone ID to assign to"),
      sprintId: z.string().optional().describe("Sprint ID to assign to"),
      editorId: z.string().optional().describe("Assigned user ID (required unless unassigned: true)"),
      unassigned: z.boolean().optional().describe(
        "Set to true ONLY when the user explicitly requested an unassigned ticket",
      ),
      tags: z.string().optional().describe("Comma-separated tags"),
      storypoints: z.string().optional().describe("Story points"),
      dateToFinish: z.string().optional().describe("Due date (YYYY-MM-DD)"),
      dependingTicketId: z.string().optional().describe("Parent ticket ID (for subtasks)"),
      planHours: z.number().optional().describe("Planned hours estimate"),
    },
    async (params) => {
      try {
        const users = await fetchUsers(client);

        if (!params.editorId && !params.unassigned) {
          return errorResult(
            `Assignment required: ${ASSIGNMENT_RULE} Then pass either editorId ` +
              `or unassigned: true (ONLY if the user explicitly opted out of assignment). ` +
              `Available users: ${usersList(users)}`,
          );
        }
        if (params.editorId && !users.some((u) => u.id === String(params.editorId))) {
          return errorResult(
            `editorId "${params.editorId}" does not exist. Available users: ${usersList(users)}`,
          );
        }

        const ticket: Record<string, unknown> = {
          headline: params.headline,
          projectId: params.projectId,
        };
        if (params.description) ticket.description = markdownToHtml(params.description);
        if (params.type) ticket.type = params.type;
        if (params.priority) ticket.priority = params.priority;
        if (params.status) ticket.status = params.status;
        if (params.milestoneId) ticket.milestoneid = params.milestoneId;
        if (params.sprintId) ticket.sprint = params.sprintId;
        if (params.editorId) ticket.editorId = params.editorId;
        if (params.tags) ticket.tags = params.tags;
        if (params.storypoints) ticket.storypoints = params.storypoints;
        if (params.dateToFinish) ticket.dateToFinish = params.dateToFinish;
        if (params.dependingTicketId) ticket.dependingTicketId = params.dependingTicketId;
        if (params.planHours) ticket.planHours = params.planHours;

        const result = await client.call<unknown>(
          "tickets.addTicket",
          { values: ticket },
        );
        // Leantime returns the new id as an array ([id]) — normalize for the agent.
        const normalized = Array.isArray(result) ? { id: result[0] } : result;
        return {
          content: [{ type: "text", text: JSON.stringify(normalized, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_update_ticket",
    `Update an existing ticket/task. Only the provided fields are changed (Leantime's ` +
      `patch API — other fields are never wiped). The description is ${MARKDOWN_HINT} and ` +
      `replaces the previous description entirely. Only set editorId when you intend to ` +
      `change the assignment (validate user IDs with leantime_list_users).`,
    {
      ticketId: z.string().describe("The ticket ID"),
      headline: z.string().optional().describe("New ticket title"),
      description: z.string().optional().describe(`New description in ${MARKDOWN_HINT}`),
      type: z.string().optional().describe("New ticket type"),
      status: z.number().optional().describe("New status ID"),
      priority: z.number().optional().describe("New priority (1-5)"),
      milestoneId: z.string().optional().describe("New milestone ID"),
      sprintId: z.string().optional().describe("New sprint ID"),
      editorId: z.string().optional().describe("New assigned user ID (validates against leantime_list_users)"),
      tags: z.string().optional().describe("New comma-separated tags"),
      storypoints: z.string().optional().describe("New story points"),
      dateToFinish: z.string().optional().describe("New due date (YYYY-MM-DD)"),
      dependingTicketId: z.string().optional().describe("Parent ticket ID (for subtasks)"),
      planHours: z.number().optional().describe("Planned hours estimate"),
    },
    async ({ ticketId, editorId, description, ...updates }) => {
      try {
        if (editorId !== undefined) {
          const users = await fetchUsers(client);
          if (!users.some((u) => u.id === String(editorId))) {
            return errorResult(
              `editorId "${editorId}" does not exist. Available users: ${usersList(users)}`,
            );
          }
        }

        const changes: Record<string, unknown> = {};
        if (updates.headline !== undefined) changes.headline = updates.headline;
        if (updates.type !== undefined) changes.type = updates.type;
        if (description !== undefined) changes.description = markdownToHtml(description);
        if (updates.status !== undefined) changes.status = updates.status;
        if (updates.priority !== undefined) changes.priority = updates.priority;
        if (updates.milestoneId !== undefined) changes.milestoneid = updates.milestoneId;
        if (updates.sprintId !== undefined) changes.sprint = updates.sprintId;
        if (editorId !== undefined) changes.editorId = editorId;
        if (updates.tags !== undefined) changes.tags = updates.tags;
        if (updates.storypoints !== undefined) changes.storypoints = updates.storypoints;
        if (updates.dateToFinish !== undefined) changes.dateToFinish = updates.dateToFinish;
        if (updates.dependingTicketId !== undefined) changes.dependingTicketId = updates.dependingTicketId;
        if (updates.planHours !== undefined) changes.planHours = updates.planHours;

        if (Object.keys(changes).length === 0) {
          return errorResult("Nothing to update: provide at least one field to change.");
        }

        const result = await client.call<boolean>(
          "tickets.patch",
          { id: ticketId, params: changes },
        );
        return {
          content: [{
            type: "text",
            text: JSON.stringify({ ok: result === true, id: ticketId }, null, 2),
          }],
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
