import { McpServer } from "@mcp/server";
import { z } from "zod";
import type { LeantimeClient } from "../leantime-client.ts";
import { markdownToHtml } from "../markdown.ts";
import {
  CONFIRM_PARAM_DOC,
  checkDestructiveConfirmation,
  errorResult,
  isLeantimeError,
  MARKDOWN_HINT,
} from "./shared.ts";

export function registerCommentTools(server: McpServer, client: LeantimeClient) {
  server.tool(
    "leantime_list_comments",
    "List the discussion comments of a ticket",
    {
      ticketId: z.string().describe("The ticket ID"),
    },
    async ({ ticketId }) => {
      try {
        // Leantime expects `entityId` (not moduleId) — param names must match exactly.
        const result = await client.call<unknown[]>("comments.getComments", {
          module: "ticket",
          entityId: ticketId,
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
    "leantime_add_comment",
    `Add a comment to a ticket's discussion. The text is ${MARKDOWN_HINT}.`,
    {
      ticketId: z.string().describe("The ticket ID"),
      text: z.string().describe(`Comment body in ${MARKDOWN_HINT}`),
      father: z.number().optional().describe(
        "Parent comment ID for a reply (omit or 0 for a top-level comment)",
      ),
    },
    async ({ ticketId, text, father }) => {
      try {
        // addComment requires the full `entity` object — rebuild it from the ticket.
        const ticket = await client.call<Record<string, unknown>>("tickets.getTicket", {
          id: ticketId,
        });
        if (typeof ticket === "boolean" || isLeantimeError(ticket)) {
          return errorResult(`Ticket ${ticketId} not found.`);
        }

        const html = markdownToHtml(text);
        try {
          await client.call<boolean>("comments.addComment", {
            values: { text: html, father: father ?? 0 },
            module: "ticket",
            entityId: ticketId,
            entity: {
              id: ticketId,
              type: ticket.type ?? "task",
              headline: ticket.headline,
            },
          });
        } catch (e) {
          // Leantime v3.7.3 bug: the comment row is inserted, then the
          // notification build crashes on entity property access via JSON-RPC.
          // Verify the comment actually landed before surfacing an error.
          const comments = await client.call<{ text?: string }[]>("comments.getComments", {
            module: "ticket",
            entityId: ticketId,
          });
          const landed = (comments ?? []).some((c) => c.text === html);
          if (!landed) throw e;
        }
        return {
          content: [{ type: "text", text: JSON.stringify({ ok: true, ticketId }, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_update_comment",
    `Edit an existing comment. The text is ${MARKDOWN_HINT}.`,
    {
      commentId: z.string().describe("The comment ID"),
      text: z.string().describe(`New comment body in ${MARKDOWN_HINT}`),
    },
    async ({ commentId, text }) => {
      try {
        const result = await client.call<boolean>("comments.editComment", {
          values: { text: markdownToHtml(text) },
          id: commentId,
        });
        return {
          content: [{ type: "text", text: JSON.stringify({ ok: result === true, commentId }, null, 2) }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );

  server.tool(
    "leantime_delete_comment",
    "Delete a comment. Destructive: requires explicit user approval (confirm: true) " +
      "unless LEANTIME_MCP_DESTRUCTIVE_POLICY is set otherwise.",
    {
      commentId: z.string().describe("The comment ID"),
      confirm: z.boolean().optional().describe(CONFIRM_PARAM_DOC),
    },
    async ({ commentId, confirm }) => {
      const check = checkDestructiveConfirmation(confirm, "comment");
      if (!check.allowed) return errorResult(check.message);
      try {
        const result = await client.call<boolean>("comments.deleteComment", {
          commentId,
        });
        return {
          content: [{
            type: "text",
            text: JSON.stringify({ deleted: result === true, commentId }, null, 2),
          }],
        };
      } catch (e) {
        return errorResult(e instanceof Error ? e.message : String(e));
      }
    },
  );
}
