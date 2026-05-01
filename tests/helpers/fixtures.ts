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
  { id: "1", name: "Sprint 1", projectId: "3", startDate: "2026-01-01", endDate: "2026-01-14" },
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
