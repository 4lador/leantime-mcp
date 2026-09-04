import type { LeantimeClient } from "../leantime-client.ts";

export function errorResult(message: string) {
  return {
    content: [{ type: "text" as const, text: `Error: ${message}` }],
    isError: true,
  };
}

export const MARKDOWN_HINT =
  "Markdown (## headings, lists, - [ ] checklists, **bold**, `code`, links) — converted to rich HTML for Leantime's editor";

export const ASSIGNMENT_RULE =
  "You MUST ask the user who the ticket should be assigned to before calling this tool.";

export interface LeantimeUser {
  id: string;
  name: string;
}

export async function fetchUsers(client: LeantimeClient): Promise<LeantimeUser[]> {
  const users = await client.getUsers();
  return users.map((u) => ({
    id: String(u.id),
    name: [u.firstname, u.lastname].filter(Boolean).join(" ").trim(),
  }));
}

export function usersList(users: LeantimeUser[]): string {
  return users.map((u) => `${u.id} (${u.name})`).join(", ");
}

/** Leantime services report errors as objects like {msg, type: "error"} — surface them as tool errors. */
export function isLeantimeError(
  result: unknown,
): result is { msg: string; type: string } {
  return typeof result === "object" && result !== null &&
    "type" in result && (result as { type: unknown }).type === "error" &&
    "msg" in result;
}

/** addTicket and friends return the new id as an array ([id]) — normalize for the agent. */
export function normalizeCreatedId(result: unknown): unknown {
  return Array.isArray(result) ? { id: result[0] } : result;
}

export type DestructivePolicy = "ask" | "deny" | "allow";

/** LEANTIME_MCP_DESTRUCTIVE_POLICY: ask (default) | deny | allow. */
export function getDestructivePolicy(): DestructivePolicy {
  const raw = (Deno.env.get("LEANTIME_MCP_DESTRUCTIVE_POLICY") ?? "ask")
    .trim().toLowerCase();
  return raw === "deny" || raw === "allow" ? raw : "ask";
}

export const CONFIRM_INSTRUCTION =
  "Confirmation required: ask the user for EXPLICIT approval to delete this" +
  " {what}, then retry with confirm: true. NEVER pass confirm: true without" +
  " the user's explicit consent.";

export type Confirmation =
  | { allowed: true }
  | { allowed: false; message: string };

export function checkDestructiveConfirmation(
  confirm: boolean | undefined,
  what: string,
): Confirmation {
  const policy = getDestructivePolicy();
  if (policy === "deny") {
    return {
      allowed: false,
      message:
        `Destructive operations are disabled on this server ` +
        `(LEANTIME_MCP_DESTRUCTIVE_POLICY=deny). Deleting this ${what} is refused.` +
        ` Ask the server administrator to change the policy if this deletion is really needed.`,
    };
  }
  if (policy === "allow") return { allowed: true };
  if (confirm !== true) {
    return { allowed: false, message: CONFIRM_INSTRUCTION.replace("{what}", ` ${what}`) };
  }
  return { allowed: true };
}

export const CONFIRM_PARAM_DOC =
  "MUST be true to actually delete (ask the user for explicit approval first)";
