import type { LeantimeStatusMap } from "../../src/types.ts";

export const STATUS_MAP: LeantimeStatusMap = {
  "0": { name: "Terminé", class: "label-success", statusType: "DONE", kanbanCol: "on", sortKey: "5" },
  "1": { name: "Bloqué", class: "label-important", statusType: "INPROGRESS", kanbanCol: "on", sortKey: "2" },
  "2": { name: "En attente de validation", class: "label-brown", statusType: "INPROGRESS", kanbanCol: "on", sortKey: "4" },
  "3": { name: "A Faire", class: "label-blue", statusType: "NEW", kanbanCol: "on", sortKey: "1" },
  "4": { name: "En cours", class: "label-warning", statusType: "INPROGRESS", kanbanCol: "on", sortKey: "3" },
};

export const TICKETS = [
  { id: "1", headline: "Task done", status: 0, projectId: "3", type: "task" },
  { id: "2", headline: "Task in progress", status: 4, projectId: "3", type: "task" },
  { id: "3", headline: "Task todo", status: 3, projectId: "3", type: "task" },
  { id: "4", headline: "Task blocked", status: 1, projectId: "3", type: "task" },
  { id: "5", headline: "Task unknown status", status: 99, projectId: "3", type: "task" },
  { id: "6", headline: "Other project task", status: 3, projectId: "4", type: "task" },
  { id: "7", headline: "A subtask", status: 3, projectId: "3", type: "task", dependingTicketId: "1" },
];

export const PROJECTS = [
  { id: "3", name: "Vision", status: "0" },
  { id: "4", name: "Modularbase", status: "0" },
];

export const MILESTONES = [
  { id: "22", headline: "PHASE 1", status: 3, projectId: "3", type: "milestone" },
  { id: "23", headline: "PHASE 2", status: 3, projectId: "3", type: "milestone" },
];

export const SPRINTS = [
  { id: "1", name: "Sprint 1", projectId: "3", startDate: "2026-01-01 00:00:00", endDate: "2026-01-14 00:00:00" },
  { id: "2", name: "Sprint 2", projectId: "3", startDate: "2026-09-01 00:00:00", endDate: "2026-09-30 00:00:00" },
  { id: "3", name: "Sprint 3", projectId: "3", startDate: "2026-10-01 00:00:00", endDate: "2026-10-14 00:00:00" },
];

export const USERS = [
  { id: 1, firstname: "Alador", lastname: "" },
  { id: 2, firstname: "LM", lastname: "" },
];

export const CLIENTS = [
  { id: "1", name: "TestClient" },
];

export const COMMENTS = [
  {
    id: "10",
    moduleId: "1",
    module: "ticket",
    text: "<p>First comment</p>",
    userId: "1",
    firstname: "Alador",
    lastname: "",
    date: "2026-09-04 10:00:00",
    commentParent: 0,
  },
  {
    id: "11",
    moduleId: "1",
    module: "ticket",
    text: "<p>Reply</p>",
    userId: "2",
    firstname: "LM",
    lastname: "",
    date: "2026-09-04 11:00:00",
    commentParent: "10",
  },
];

export const TIMESHEETS = [
  {
    id: "50",
    userId: "1",
    ticketId: "1",
    workDate: "2026-09-04 00:00:00",
    hours: 2.5,
    kind: "GENERAL_BILLABLE",
    description: "work",
  },
];

export const RPC_OK = <T>(result: T) => ({
  jsonrpc: "2.0",
  result,
  id: 1,
});

export const RPC_ERROR = (code: number, message: string, data?: string) => ({
  jsonrpc: "2.0",
  error: { code, message, data },
  id: 1,
});
