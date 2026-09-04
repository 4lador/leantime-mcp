import { McpServer } from "@mcp/server";
import { z } from "zod";
import type { LeantimeClient } from "../leantime-client.ts";
import { markdownToHtml } from "../markdown.ts";
import { errorResult, isLeantimeError, MARKDOWN_HINT } from "./shared.ts";

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

  server.tool(
    "leantime_create_project",
    `Create a new project. The details field is ${MARKDOWN_HINT}. ` +
      `Get a valid clientId with leantime_list_clients first.`,
    {
      name: z.string().describe("Project name"),
      clientId: z.string().describe("Client ID (see leantime_list_clients)"),
      details: z.string().optional().describe(`Project details in ${MARKDOWN_HINT}`),
      hourBudget: z.number().optional().describe("Hour budget"),
      dollarBudget: z.number().optional().describe("Dollar budget"),
    },
    async ({ name, clientId, details, hourBudget, dollarBudget }) => {
      try {
        const values: Record<string, unknown> = { name, clientId };
        if (details) values.details = markdownToHtml(details);
        if (hourBudget !== undefined) values.hourBudget = hourBudget;
        if (dollarBudget !== undefined) values.dollarBudget = dollarBudget;

        const result = await client.call<unknown>("projects.addProject", { values });
        if (result === false || result === null || isLeantimeError(result)) {
          return errorResult("Project creation failed.");
        }
        // addProject returns the new id wrapped in an array — normalize.
        const normalized = Array.isArray(result) ? { id: result[0] } : { id: result };
        return {
          content: [{ type: "text", text: JSON.stringify(normalized, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_update_project",
    `Update a project. Only the provided fields are changed (patch API — other ` +
      `fields are never wiped). The details field is ${MARKDOWN_HINT}.`,
    {
      projectId: z.string().describe("The project ID"),
      name: z.string().optional().describe("New project name"),
      details: z.string().optional().describe(`New details in ${MARKDOWN_HINT}`),
      hourBudget: z.number().optional().describe("New hour budget"),
      dollarBudget: z.number().optional().describe("New dollar budget"),
    },
    async ({ projectId, name, details, hourBudget, dollarBudget }) => {
      try {
        const params: Record<string, unknown> = {};
        if (name !== undefined) params.name = name;
        if (details !== undefined) params.details = markdownToHtml(details);
        if (hourBudget !== undefined) params.hourBudget = hourBudget;
        if (dollarBudget !== undefined) params.dollarBudget = dollarBudget;

        if (Object.keys(params).length === 0) {
          return errorResult("Nothing to update: provide at least one field to change.");
        }

        const result = await client.call<boolean>("projects.patch", {
          id: projectId,
          params,
        });
        return {
          content: [{
            type: "text",
            text: JSON.stringify({ ok: result === true, id: projectId }, null, 2),
          }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_find_projects",
    "Search projects by name (fuzzy)",
    {
      term: z.string().describe("Search term"),
    },
    async ({ term }) => {
      try {
        const result = await client.call<Record<string, unknown>[]>(
          "projects.findProject",
          { term },
        );
        // findProject mangles ids into "id-modified" — normalize back to plain ids.
        const normalized = (result ?? []).map((p) => ({
          ...p,
          id: String(p.id).split("-")[0],
        }));
        return {
          content: [{ type: "text", text: JSON.stringify(normalized, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_list_project_users",
    "List the users assigned to a project (id, name) — the valid editorId " +
      "candidates for tickets and milestones of that project",
    {
      projectId: z.string().describe("The project ID"),
    },
    async ({ projectId }) => {
      try {
        const users = await client.call<Record<string, unknown>[]>(
          "projects.getUsersAssignedToProject",
          { projectId },
        );
        const simplified = (users ?? []).map((u) => ({
          id: String(u.id),
          name: [u.firstname, u.lastname].filter(Boolean).join(" ").trim(),
          role: u.projectRole ?? undefined,
        }));
        return {
          content: [{ type: "text", text: JSON.stringify(simplified, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_list_clients",
    "List all clients (id, name) — clientId is required to create projects",
    {},
    async () => {
      try {
        const clients = await client.call<Record<string, unknown>[]>("Clients.getAll", {});
        const simplified = (clients ?? []).map((c) => ({
          id: String(c.id),
          name: c.name,
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
