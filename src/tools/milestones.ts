import { McpServer } from "@mcp/server";
import { z } from "zod";
import type { LeantimeClient } from "../leantime-client.ts";
import { markdownToHtml } from "../markdown.ts";
import {
  ASSIGNMENT_RULE,
  CONFIRM_PARAM_DOC,
  checkDestructiveConfirmation,
  errorResult,
  fetchUsers,
  isLeantimeError,
  MARKDOWN_HINT,
  normalizeCreatedId,
  usersList,
} from "./shared.ts";

function errorResultLocal(message: string) {
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
        return errorResultLocal(e instanceof Error ? e.message : String(e));
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
        return errorResultLocal(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_create_milestone",
    `Create a milestone in a project. The description is ${MARKDOWN_HINT}. ` +
      `${ASSIGNMENT_RULE} Pass the chosen editorId (get candidates with leantime_list_users), ` +
      `or pass unassigned: true ONLY if the user explicitly said to leave it unassigned.`,
    {
      projectId: z.string().describe("The project ID"),
      headline: z.string().describe("Milestone title"),
      description: z.string().optional().describe(`Milestone description in ${MARKDOWN_HINT}`),
      editorId: z.string().optional().describe("Assigned user ID (required unless unassigned: true)"),
      unassigned: z.boolean().optional().describe(
        "Set to true ONLY when the user explicitly requested an unassigned milestone",
      ),
      dateToFinish: z.string().optional().describe("Due date (YYYY-MM-DD)"),
      dependentMilestone: z.string().optional().describe("Parent milestone ID"),
    },
    async (params) => {
      try {
        const users = await fetchUsers(client);

        if (!params.editorId && !params.unassigned) {
          return errorResultLocal(
            `Assignment required: ${ASSIGNMENT_RULE} Then pass either editorId ` +
              `or unassigned: true (ONLY if the user explicitly opted out of assignment). ` +
              `Available users: ${usersList(users)}`,
          );
        }
        if (params.editorId && !users.some((u) => u.id === String(params.editorId))) {
          return errorResultLocal(
            `editorId "${params.editorId}" does not exist. Available users: ${usersList(users)}`,
          );
        }

        // Milestones are zp_tickets rows: addTicket with type "milestone" gives the
        // richest field support (quickAddMilestone drops the description entirely).
        const values: Record<string, unknown> = {
          headline: params.headline,
          type: "milestone",
          projectId: params.projectId,
        };
        if (params.description) values.description = markdownToHtml(params.description);
        if (params.editorId) values.editorId = params.editorId;
        if (params.dateToFinish) values.dateToFinish = params.dateToFinish;
        if (params.dependentMilestone) values.milestoneid = params.dependentMilestone;

        const result = await client.call<unknown>("tickets.addTicket", { values });
        if (isLeantimeError(result)) return errorResultLocal(result.msg);
        return {
          content: [{
            type: "text",
            text: JSON.stringify(normalizeCreatedId(result), null, 2),
          }],
        };
      } catch (e) {
        return errorResultLocal(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_update_milestone",
    `Update an existing milestone. Only the provided fields are changed (Leantime's ` +
      `patch API — other fields are never wiped). The description is ${MARKDOWN_HINT}.`,
    {
      milestoneId: z.string().describe("The milestone ID"),
      headline: z.string().optional().describe("New milestone title"),
      description: z.string().optional().describe(`New description in ${MARKDOWN_HINT}`),
      editorId: z.string().optional().describe(
        "New assigned user ID (validates against leantime_list_users)",
      ),
      status: z.number().optional().describe("New status ID"),
      dateToFinish: z.string().optional().describe("New due date (YYYY-MM-DD)"),
      dependentMilestone: z.string().optional().describe("New parent milestone ID"),
    },
    async ({ milestoneId, editorId, description, ...rest }) => {
      try {
        if (editorId !== undefined) {
          const users = await fetchUsers(client);
          if (!users.some((u) => u.id === String(editorId))) {
            return errorResultLocal(
              `editorId "${editorId}" does not exist. Available users: ${usersList(users)}`,
            );
          }
        }

        const changes: Record<string, unknown> = {};
        if (rest.headline !== undefined) changes.headline = rest.headline;
        if (description !== undefined) changes.description = markdownToHtml(description);
        if (editorId !== undefined) changes.editorId = editorId;
        if (rest.status !== undefined) changes.status = rest.status;
        if (rest.dateToFinish !== undefined) changes.dateToFinish = rest.dateToFinish;
        if (rest.dependentMilestone !== undefined) changes.milestoneid = rest.dependentMilestone;

        if (Object.keys(changes).length === 0) {
          return errorResultLocal("Nothing to update: provide at least one field to change.");
        }

        // quickUpdateMilestone reads projectId from the session (unset for API
        // keys) — use the safe generic ticket patch instead.
        const result = await client.call<boolean>("tickets.patch", {
          id: milestoneId,
          params: changes,
        });
        return {
          content: [{
            type: "text",
            text: JSON.stringify({ ok: result === true, id: milestoneId }, null, 2),
          }],
        };
      } catch (e) {
        return errorResultLocal(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_get_milestone_progress",
    "Get the completion percentage of a milestone (weighted by effort and priority " +
      "of its tickets, mirroring Leantime's own formula)",
    {
      milestoneId: z.string().describe("The milestone ID"),
    },
    async ({ milestoneId }) => {
      try {
        // Leantime's getMilestoneProgress takes a union-typed param (int|string)
        // that its own JSON-RPC binder cannot cast — compute the same formula here.
        const milestone = await client.call<Record<string, unknown>>(
          "tickets.getTicket",
          { id: milestoneId },
        );
        if (typeof milestone === "boolean" || isLeantimeError(milestone)) {
          return errorResultLocal(`Milestone ${milestoneId} not found.`);
        }
        const projectId = String(milestone.projectId);

        const tickets = await client.call<Record<string, unknown>[]>("tickets.getAll", {
          searchCriteria: { milestone: milestoneId, currentProject: projectId },
          limit: 500,
        });
        const statusLabels = await client.call<Record<string, Record<string, unknown>>>(
          "tickets.getStatusLabels",
          { projectId },
        );

        const priorityFactor: Record<string, number> = {
          "1": 2, "2": 1.75, "3": 1.5, "4": 1.25, "5": 1,
        };
        const defaultEffort = 3, defaultPriority = 3;
        let totalScore = 0, doneScore = 0;
        for (const t of tickets ?? []) {
          const effort = t.storypoints === undefined || t.storypoints === "" || t.storypoints === null
            ? defaultEffort
            : Number(t.storypoints);
          const priority = t.priority === undefined || t.priority === "" || t.priority === null
            ? defaultPriority
            : Number(t.priority);
          const score = effort * (priorityFactor[String(priority)] ?? 1);
          totalScore += score;
          const label = statusLabels[String(t.status)];
          if (label && label.statusType === "DONE") doneScore += score;
        }
        const percentDone = totalScore === 0 ? 0 : (doneScore / totalScore) * 100;

        return {
          content: [{
            type: "text",
            text: JSON.stringify({
              milestoneId,
              percentDone: Math.round(percentDone * 10) / 10,
              tickets: (tickets ?? []).length,
            }, null, 2),
          }],
        };
      } catch (e) {
        return errorResultLocal(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_delete_milestone",
    "Delete a milestone (its tickets are kept). Destructive: requires explicit user " +
      "approval (confirm: true) unless LEANTIME_MCP_DESTRUCTIVE_POLICY is set otherwise.",
    {
      milestoneId: z.string().describe("The milestone ID"),
      confirm: z.boolean().optional().describe(CONFIRM_PARAM_DOC),
    },
    async ({ milestoneId, confirm }) => {
      const check = checkDestructiveConfirmation(confirm, "milestone");
      if (!check.allowed) return errorResultLocal(check.message);
      try {
        const result = await client.call<unknown>("tickets.deleteMilestone", {
          id: milestoneId,
        });
        if (isLeantimeError(result)) return errorResultLocal(result.msg);
        return {
          content: [{
            type: "text",
            text: JSON.stringify({ deleted: true, milestoneId }, null, 2),
          }],
        };
      } catch (e) {
        return errorResultLocal(e instanceof Error ? e.message : String(e));
      }
    },
  );
}
