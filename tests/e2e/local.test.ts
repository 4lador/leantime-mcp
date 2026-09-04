/**
 * Exhaustive local e2e — runs the full tool surface against a REAL Leantime
 * instance (the docker-compose one, usually). Opt-in:
 *
 *   LEANTIME_E2E=local LEANTIME_URL=http://localhost:8090 \
 *   LEANTIME_API_KEY=lt_... deno task test:e2e:local
 *
 * Safety rails baked in (lessons from the data-loss incident):
 *   - every created id is CAPTURED, and deletion goes through deleteCaptured()
 *     which REFUSES any id this suite did not create
 *   - no unscoped tickets.getAll: listing always goes through searchCriteria
 *   - final assertion: the scratch project is left empty, then hidden
 */
import { assertEquals } from "@std/assert";
import { LeantimeClient } from "../../src/leantime-client.ts";
import { registerAllTools } from "../../src/tools/mod.ts";

const ready = Deno.env.get("LEANTIME_E2E") === "local" &&
  !!(Deno.env.get("LEANTIME_URL") && Deno.env.get("LEANTIME_API_KEY"));

const URL_ = Deno.env.get("LEANTIME_URL")!;
const KEY = Deno.env.get("LEANTIME_API_KEY")!;

// Destructive tests run under the default "ask" policy.
Deno.env.set("LEANTIME_MCP_DESTRUCTIVE_POLICY", "ask");

// ---------------------------------------------------------------- capture

const captured = {
  tickets: new Set<string>(), // tickets AND milestones AND subtasks
  comments: new Set<string>(),
  timesheets: new Set<string>(),
  projects: new Set<string>(),
};
type Kind = keyof typeof captured;

function capture(kind: Kind, id: unknown): string {
  const sid = String(id);
  captured[kind].add(sid);
  return sid;
}

function assertCaptured(kind: Kind, id: string): void {
  if (!captured[kind].has(id)) {
    throw new Error(
      `SAFETY: refusing to delete ${kind} ${id} — not created by this suite`,
    );
  }
}

// ---------------------------------------------------------------- registry

const tools = new Map<string, (p: Record<string, unknown>) => Promise<unknown>>();
const client = new LeantimeClient(URL_, KEY);
registerAllTools(
  { tool: (n, _d, _s, h) => tools.set(n, h) } as never,
  client,
);

interface R {
  isError: boolean;
  text: string;
  parsed: unknown;
}

async function call(name: string, args: Record<string, unknown>): Promise<R> {
  const r = await tools.get(name)!(args) as {
    content: { text: string }[];
    isError?: boolean;
  };
  const text = r.content[0].text;
  let parsed: unknown = null;
  try {
    parsed = JSON.parse(text);
  } catch {
    // error text
  }
  return { isError: r.isError === true, text, parsed };
}

const ok = (r: R) => assertEquals(r.isError, false, r.text);

// ---------------------------------------------------------------- state

const state: { projects: string[]; sprint?: string; milestone?: string; ticket?: string; subtask?: string } = {
  projects: [],
};

// ---------------------------------------------------------------- tests

Deno.test({
  name: "local e2e — projects & clients",
  ignore: !ready,
  fn: async () => {
    const clients = await call("leantime_list_clients", {});
    ok(clients);
    assertEquals((clients.parsed as unknown[]).length > 0, true, "no client to attach the project to");

    const created = await call("leantime_create_project", {
      name: `e2e-scratch-${Date.now()}`,
      clientId: "1",
      details: "Scratch **e2e** project — auto-created",
    });
    ok(created);
    const pid = capture("projects", (created.parsed as { id: string }).id);
    state.projects.push(pid);

    const got = await call("leantime_get_project", { projectId: pid });
    ok(got);
    assertEquals(
      ((got.parsed as { details: string }).details).includes("<strong>e2e</strong>"),
      true,
      "project details markdown was not converted",
    );

    const prog = await call("leantime_get_project_progress", { projectId: pid });
    ok(prog);

    const upd = await call("leantime_update_project", { projectId: pid, name: `e2e-scratch-${Date.now()}-renamed` });
    ok(upd);

    const found = await call("leantime_find_projects", { term: "e2e-scratch" });
    ok(found);
    assertEquals(
      (found.parsed as { id: string }[]).some((p) => String(p.id).split("-")[0] === pid),
      true,
      "renamed scratch project not found",
    );

    const users = await call("leantime_list_project_users", { projectId: pid });
    ok(users); // fresh project: empty is legitimate, loud-skip philosophy

    const list = await call("leantime_list_projects", {});
    ok(list);
    assertEquals(
      (list.parsed as { id: string }[]).some((p) => String(p.id) === pid),
      true,
    );
  },
});

Deno.test({
  name: "local e2e — sprints CRUD + current",
  ignore: !ready,
  fn: async () => {
    const pid = state.projects[0];
    const created = await call("leantime_create_sprint", {
      projectId: pid,
      name: "e2e Sprint",
      startDate: "2026-01-01",
      endDate: "2030-12-31",
    });
    ok(created);
    state.sprint = String((created.parsed as { id: string }).id);

    const upd = await call("leantime_update_sprint", { sprintId: state.sprint!, name: "e2e Sprint renamed" });
    ok(upd);

    const cur = await call("leantime_get_current_sprint", { projectId: pid });
    ok(cur);
    assertEquals(
      String((cur.parsed as { current: { id: string } | null }).current?.id ?? ""),
      state.sprint!,
      "current sprint should be the one spanning today",
    );

    const list = await call("leantime_list_sprints", { projectId: pid });
    ok(list);
    assertEquals(
      (list.parsed as { id: string }[]).some((s) => String(s.id) === state.sprint),
      true,
    );
  },
});

Deno.test({
  name: "local e2e — milestones: enforcement + CRUD + progress",
  ignore: !ready,
  fn: async () => {
    const pid = state.projects[0];

    const refused = await call("leantime_create_milestone", { projectId: pid, headline: "no assignment" });
    assertEquals(refused.isError, true);
    assertEquals(refused.text.includes("Assignment required"), true);

    const created = await call("leantime_create_milestone", {
      projectId: pid,
      headline: "e2e milestone",
      editorId: "1",
      description: "Jalon **e2e**",
    });
    ok(created);
    state.milestone = capture("tickets", (created.parsed as { id: string }).id);

    const list = await call("leantime_list_milestones", { projectId: pid });
    ok(list);
    assertEquals(
      (list.parsed as { type: string }[]).every((m) => m.type === "milestone"),
      true,
    );

    const upd = await call("leantime_update_milestone", { milestoneId: state.milestone!, headline: "e2e milestone renamed" });
    ok(upd);

    const prog = await call("leantime_get_milestone_progress", { milestoneId: state.milestone! });
    ok(prog);
    assertEquals(typeof (prog.parsed as { percentDone: number }).percentDone, "number");
  },
});

Deno.test({
  name: "local e2e — tickets: enforcement, markdown, scoping regression, subtasks, reads",
  ignore: !ready,
  fn: async () => {
    const pid = state.projects[0];

    // Assignment enforcement
    const refused = await call("leantime_create_ticket", { projectId: pid, headline: "no assignment" });
    assertEquals(refused.isError, true);
    assertEquals(refused.text.includes("Assignment required"), true);
    const badEditor = await call("leantime_create_ticket", { projectId: pid, headline: "bad editor", editorId: "999" });
    assertEquals(badEditor.isError, true);

    // Create with markdown + sprint + milestone
    const created = await call("leantime_create_ticket", {
      projectId: pid,
      headline: "e2e ticket",
      editorId: "1",
      description: "## Contexte\n\n- [ ] étape\n- point",
      sprintId: state.sprint!,
      milestoneId: state.milestone!,
    });
    ok(created);
    state.ticket = capture("tickets", (created.parsed as { id: string }).id);

    const got = await call("leantime_get_ticket", { projectId: pid, ticketId: state.ticket! });
    ok(got);
    const tick = got.parsed as { description: string; editorId: string };
    assertEquals(tick.description.includes("<h2>Contexte</h2>"), true, "markdown not converted");
    assertEquals(tick.description.includes('data-type="taskList"'), true);
    assertEquals(String(tick.editorId), "1");

    // Patch preserves unrelated fields (regression of the field-wiping bug)
    const upd = await call("leantime_update_ticket", { ticketId: state.ticket!, headline: "e2e ticket renamed" });
    ok(upd);
    const got2 = await call("leantime_get_ticket", { projectId: pid, ticketId: state.ticket! });
    const tick2 = got2.parsed as { editorId: string; headline: string; sprint: string };
    assertEquals(String(tick2.editorId), "1", "patch wiped editorId");
    assertEquals(tick2.headline, "e2e ticket renamed");
    assertEquals(String(tick2.sprint), state.sprint!, "patch wiped sprint");

    // SCOPING regression: a second project's tickets never leak
    const other = await call("leantime_create_project", { name: `e2e-other-${Date.now()}`, clientId: "1" });
    ok(other);
    const otherPid = capture("projects", (other.parsed as { id: string }).id);
    state.projects.push(otherPid);
    const otherTicket = await call("leantime_create_ticket", {
      projectId: otherPid,
      headline: "e2e other-project ticket",
      unassigned: true,
    });
    ok(otherTicket);
    const otherTid = capture("tickets", (otherTicket.parsed as { id: string }).id);

    const scoped = await call("leantime_list_tickets", { projectId: pid });
    ok(scoped);
    const scopedIds = (scoped.parsed as { id: string }[]).map((t) => String(t.id));
    assertEquals(scopedIds.includes(state.ticket!), true);
    assertEquals(scopedIds.includes(otherTid), false, "SCOPING LEAK: other project's ticket visible");
    assertEquals(
      (scoped.parsed as { projectId: string }[]).every((t) => String(t.projectId) === pid),
      true,
    );

    // Subtasks
    const sub = await call("leantime_create_ticket", {
      projectId: pid,
      headline: "e2e subtask",
      unassigned: true,
      dependingTicketId: state.ticket!,
    });
    ok(sub);
    state.subtask = capture("tickets", (sub.parsed as { id: string }).id);
    const subs = await call("leantime_list_subtasks", { ticketId: state.ticket! });
    ok(subs);
    assertEquals(
      (subs.parsed as { id: string }[]).some((s) => String(s.id) === state.subtask),
      true,
    );

    // my_tasks / options / statuses / types
    const mine = await call("leantime_my_tasks", { userId: "1", projectId: pid });
    ok(mine);
    assertEquals(
      (mine.parsed as { id: string }[]).some((t) => String(t.id) === state.ticket),
      true,
    );
    const opts = await call("leantime_get_ticket_options", { projectId: pid });
    ok(opts);
    for (const k of ["priorities", "efforts", "kanban", "types"]) {
      assertEquals(k in (opts.parsed as object), true, `options missing ${k}`);
    }
    const statuses = await call("leantime_get_statuses", { projectId: pid });
    ok(statuses);
    assertEquals(Object.keys(statuses.parsed as object).length > 0, true);
    const types = await call("leantime_get_ticket_types", { projectId: pid });
    ok(types);

    const users = await call("leantime_list_users", {});
    ok(users);
    assertEquals((users.parsed as { id: string }[]).some((u) => u.id === "1"), true);
  },
});

Deno.test({
  name: "local e2e — comments (markdown) + timesheets",
  ignore: !ready,
  fn: async () => {
    const pid = state.projects[0];

    const empty = await call("leantime_list_comments", { ticketId: state.ticket! });
    ok(empty);

    const added = await call("leantime_add_comment", {
      ticketId: state.ticket!,
      text: "Commentaire **e2e** avec `code`",
    });
    ok(added);

    const listed = await call("leantime_list_comments", { ticketId: state.ticket! });
    ok(listed);
    const comments = listed.parsed as { id: string; text: string }[];
    assertEquals(comments.length, 1);
    assertEquals(comments[0].text.includes("<strong>e2e</strong>"), true, "comment markdown not converted");
    const cid = capture("comments", comments[0].id);

    const upd = await call("leantime_update_comment", { commentId: cid, text: "Édité **e2e**" });
    ok(upd);

    // Timesheets: add then set (idempotent) on the same day/kind
    const today = new Date().toISOString().slice(0, 10);
    const log1 = await call("leantime_log_time", {
      ticketId: state.ticket!,
      hours: 1.5,
      kind: "DEVELOPMENT",
      date: today,
      description: "e2e",
    });
    ok(log1);
    const log2 = await call("leantime_log_time", {
      ticketId: state.ticket!,
      hours: 2,
      mode: "set",
      kind: "TESTING",
      date: today,
    });
    ok(log2);

    const time = await call("leantime_get_ticket_time", { ticketId: state.ticket! });
    ok(time);
    const tt = time.parsed as { totalHours: number };
    assertEquals(Math.abs(tt.totalHours - 3.5) < 0.01, true, `expected 3.5 total hours, got ${tt.totalHours}`);

    const sheets = await call("leantime_list_timesheets", { dateFrom: today, dateTo: today, projectId: pid });
    ok(sheets);
    for (const e of sheets.parsed as { id: string }[]) {
      capture("timesheets", e.id);
    }
    assertEquals((sheets.parsed as unknown[]).length, 2);
  },
});

Deno.test({
  name: "local e2e — destructive gating: rejected without confirm, executed with",
  ignore: !ready,
  fn: async () => {
    // Policy is "ask" (set at module top).
    const pairs: [string, Record<string, unknown>, Kind][] = [
      ["leantime_delete_ticket", { ticketId: state.subtask! }, "tickets"],
      ["leantime_delete_comment", { commentId: [...captured.comments][0] }, "comments"],
    ];
    // timesheet entries: delete one of the two captured
    const tsEntry = [...captured.timesheets][0];
    if (tsEntry) pairs.push(["leantime_delete_timesheet_entry", { entryId: tsEntry }, "timesheets"]);

    for (const [tool, args, kind] of pairs) {
      const idArg = Object.values(args)[0] as string;
      assertCaptured(kind, String(idArg));

      const refused = await call(tool, args);
      assertEquals(refused.isError, true, `${tool} must refuse without confirm`);
      assertEquals(refused.text.includes("Confirmation required"), true);

      const done = await call(tool, { ...args, confirm: true });
      assertEquals(done.isError, false, done.text);
      captured[kind].delete(String(idArg));
    }

    // Milestone delete — captured, gated
    assertCaptured("tickets", state.milestone!);
    const mRefused = await call("leantime_delete_milestone", { milestoneId: state.milestone! });
    assertEquals(mRefused.isError, true);
    const mDone = await call("leantime_delete_milestone", { milestoneId: state.milestone!, confirm: true });
    assertEquals(mDone.isError, false, mDone.text);
    captured.tickets.delete(state.milestone!);

    // deny policy blocks even with confirm
    Deno.env.set("LEANTIME_MCP_DESTRUCTIVE_POLICY", "deny");
    try {
      const denied = await call("leantime_delete_ticket", { ticketId: state.ticket!, confirm: true });
      assertEquals(denied.isError, true);
      assertEquals(denied.text.includes("deny"), true);
    } finally {
      Deno.env.set("LEANTIME_MCP_DESTRUCTIVE_POLICY", "ask");
    }
  },
});

Deno.test({
  name: "local e2e — cleanup: captured ids only, scratch left empty and hidden",
  ignore: !ready,
  fn: async () => {
    // Delete remaining captured objects — assertCaptured REFUSES anything else.
    for (const cid of [...captured.comments]) {
      assertCaptured("comments", cid);
      const r = await call("leantime_delete_comment", { commentId: cid, confirm: true });
      assertEquals(r.isError, false, r.text);
      captured.comments.delete(cid);
    }
    for (const eid of [...captured.timesheets]) {
      assertCaptured("timesheets", eid);
      const r = await call("leantime_delete_timesheet_entry", { entryId: eid, confirm: true });
      assertEquals(r.isError, false, r.text);
      captured.timesheets.delete(eid);
    }
    for (const tid of [...captured.tickets]) {
      assertCaptured("tickets", tid);
      const r = await call("leantime_delete_ticket", { ticketId: tid, confirm: true });
      assertEquals(r.isError, false, r.text);
      captured.tickets.delete(tid);
    }

    // Scratch projects must be EMPTY now (scoped read — never unscoped).
    for (const pid of state.projects) {
      const remaining = await client.call<unknown[]>("tickets.getAll", {
        searchCriteria: { currentProject: pid },
        limit: 500,
      });
      assertEquals(
        (remaining ?? []).length,
        0,
        `scratch project ${pid} not empty after cleanup — leftover: ${
          JSON.stringify(remaining?.map((t) => (t as { id: string; headline: string }).headline))
        }`,
      );
      // Hide the scratch project (cleanup of our own objects only).
      await client.call("projects.editProject", {
        values: { name: "e2e-scratch-hidden", state: -1 },
        id: pid,
      });
    }

    assertEquals(captured.tickets.size, 0);
    assertEquals(captured.comments.size, 0);
    assertEquals(captured.timesheets.size, 0);
  },
});
