import { McpServer } from "@mcp/server";
import { z } from "zod";
import type { LeantimeClient } from "../leantime-client.ts";
import {
  CONFIRM_PARAM_DOC,
  checkDestructiveConfirmation,
  errorResult,
  isLeantimeError,
} from "./shared.ts";

export const HOUR_KINDS = [
  "GENERAL_BILLABLE",
  "GENERAL_NOT_BILLABLE",
  "PROJECTMANAGEMENT",
  "DEVELOPMENT",
  "BUGFIXING_NOT_BILLABLE",
  "TESTING",
] as const;

function today(): string {
  return new Date().toISOString().slice(0, 10);
}

export function registerTimesheetTools(server: McpServer, client: LeantimeClient) {
  server.tool(
    "leantime_log_time",
    "Log time on a ticket. mode \"add\" accumulates hours (default); mode \"set\" " +
      "is idempotent (sets the total for that day/kind).",
    {
      ticketId: z.string().describe("The ticket ID"),
      hours: z.number().positive().describe("Hours to log"),
      kind: z.enum(HOUR_KINDS).optional().describe(
        "Hour type (default GENERAL_BILLABLE)",
      ),
      date: z.string().optional().describe("Work date, YYYY-MM-DD (default today)"),
      description: z.string().optional().describe("What was done"),
      mode: z.enum(["add", "set"]).optional().describe(
        'add = accumulate (logTime), set = idempotent total (upsertTime). Default "add"',
      ),
    },
    async ({ ticketId, hours, kind, date, description, mode }) => {
      try {
        const method = mode === "set" ? "timesheets.upsertTime" : "timesheets.logTime";
        const params = {
          kind: kind ?? "GENERAL_BILLABLE",
          hours,
          date: date ?? today(),
          ...(description ? { description } : {}),
        };
        const result = await client.call<unknown>(method, { ticketId, params });
        if (isLeantimeError(result)) {
          return errorResult(result.msg);
        }
        return {
          content: [{
            type: "text",
            text: JSON.stringify({ ok: true, ticketId, mode: mode ?? "add", ...params }, null, 2),
          }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_get_ticket_time",
    "Get time booked on a ticket: total hours and per-day breakdown",
    {
      ticketId: z.string().describe("The ticket ID"),
    },
    async ({ ticketId }) => {
      try {
        const totalRaw = await client.call<unknown>(
          "timesheets.getSumLoggedHoursForTicket",
          { ticketId },
        );
        // The API wraps the sum in an array — unwrap.
        const total = Array.isArray(totalRaw) ? totalRaw[0] : totalRaw;
        const byDate = await client.call<unknown>(
          "timesheets.getLoggedHoursForTicketByDate",
          { ticketId },
        );
        return {
          content: [{
            type: "text",
            text: JSON.stringify({ totalHours: total, byDate: byDate ?? [] }, null, 2),
          }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_list_timesheets",
    "List booked time entries between two dates (all projects or one project)",
    {
      dateFrom: z.string().describe("Start date, YYYY-MM-DD"),
      dateTo: z.string().describe("End date, YYYY-MM-DD"),
      projectId: z.string().optional().describe("Restrict to one project"),
    },
    async ({ dateFrom, dateTo, projectId }) => {
      try {
        // A bare YYYY-MM-DD dateTo means "end of that day" — Leantime's
        // whereBetween would otherwise exclude everything after 00:00:00.
        const dateToEnd = /^\d{4}-\d{2}-\d{2}$/.test(dateTo)
          ? `${dateTo} 23:59:59`
          : dateTo;
        const result = await client.call<unknown[]>("timesheets.getAll", {
          dateFrom,
          dateTo: dateToEnd,
          ...(projectId ? { projectId } : {}),
        });
        return {
          content: [{ type: "text", text: JSON.stringify(result ?? [], null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_delete_timesheet_entry",
    "Delete a booked time entry. Destructive: requires explicit user approval " +
      "(confirm: true) unless LEANTIME_MCP_DESTRUCTIVE_POLICY is set otherwise.",
    {
      entryId: z.string().describe("The timesheet entry ID"),
      confirm: z.boolean().optional().describe(CONFIRM_PARAM_DOC),
    },
    async ({ entryId, confirm }) => {
      const check = checkDestructiveConfirmation(confirm, "timesheet entry");
      if (!check.allowed) return errorResult(check.message);
      try {
        await client.call<unknown>("timesheets.deleteTime", { id: entryId });
        return {
          content: [{
            type: "text",
            text: JSON.stringify({ deleted: true, entryId }, null, 2),
          }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );
}
