export interface JsonRpcRequest {
  jsonrpc: "2.0";
  method: string;
  params?: Record<string, unknown>;
  id: number;
}

export interface JsonRpcResponse<T = unknown> {
  jsonrpc: "2.0";
  result?: T;
  error?: {
    code: number;
    message: string;
    data?: string;
  };
  id: number;
}

export interface LeantimeProject {
  id: string;
  name: string;
  details: string;
  status: string;
  clientId: string;
  hourBudget: string;
  assignedUsers: string;
  sprint: string;
  start: string;
  end: string;
  modified: string;
  parentId: string | null;
  type: string;
  [key: string]: unknown;
}

export interface LeantimeTicket {
  id: string;
  headline: string;
  description: string;
  status: number;
  milestoneid: string;
  sprint: string;
  projectId: string;
  editorId: string;
  userId: string;
  priority: number;
  date: string;
  dateToFinish: string;
  sortindex: number;
  storypoints: string;
  hourRemaining: string;
  planHours: string;
  type: string;
  tags: string;
  dependingTicketId: string;
  editorFirstname: string;
  editorLastname: string;
  userFirstname: string;
  userLastname: string;
  milestoneHeadline: string;
  [key: string]: unknown;
}

export interface LeantimeMilestone {
  id: string;
  headline: string;
  description: string;
  status: number;
  projectId: string;
  dateToFinish: string;
  editFrom: string;
  editTo: string;
  progress: number;
  [key: string]: unknown;
}

export interface LeantimeSprint {
  id: string;
  name: string;
  projectId: string;
  startDate: string;
  endDate: string;
  [key: string]: unknown;
}

export interface LeantimeStatusLabel {
  id: number;
  name: string;
  color: string;
  [key: string]: unknown;
}

export interface LeantimeStatusEntry {
  name: string;
  class: string;
  statusType: string;
  kanbanCol: string | boolean;
  sortKey: string;
  [key: string]: unknown;
}

export type LeantimeStatusMap = Record<string, LeantimeStatusEntry>;

export interface LeantimeProjectProgress {
  percent: number;
  totalTickets: number;
  doneTickets: number;
  openTickets: number;
  [key: string]: unknown;
}
