import type {
  AgentConversation,
  AgentConversationMutationResult,
  AgentExecution,
  AgentExecutionChangeSet,
  AgentExecutionEvent,
  AgentExecutionEventsPage,
  AgentExecutionMutationResult,
} from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const CONVERSATION_COLUMNS: readonly TableColumn<AgentConversation>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'STATUS', value: (row) => row.status },
  { header: 'EVENT HEAD', value: (row) => row.lastEventSequence },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
];

const EXECUTION_COLUMNS: readonly TableColumn<AgentExecution>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'CONVERSATION', value: (row) => row.conversationId },
  { header: 'AGENT RELEASE', value: (row) => row.agent.assetReleaseId },
  { header: 'STATUS', value: (row) => row.status },
  { header: 'OPERATION', value: (row) => row.operationId },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
  { header: 'FAILURE', value: (row) => row.failure },
];

export function agentConversationsResult(rows: AgentConversation[]): CommandResult {
  return listResult(rows, CONVERSATION_COLUMNS);
}

export function agentConversationResult(row: AgentConversation): CommandResult {
  return singleResult(row, CONVERSATION_COLUMNS);
}

export function agentConversationMutationResult(row: AgentConversationMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [{ ...row.conversation, replayed: row.replayed }],
      [...CONVERSATION_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

export function agentExecutionsResult(rows: AgentExecution[]): CommandResult {
  return listResult(rows, EXECUTION_COLUMNS);
}

export function agentExecutionResult(row: AgentExecution): CommandResult {
  return singleResult(row, EXECUTION_COLUMNS);
}

export function agentExecutionChangeSetResult(row: AgentExecutionChangeSet): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'EXECUTION', value: (value) => value.executionId },
        { header: 'STATE', value: (value) => value.changeSet.state },
        { header: 'BASE TREE', value: (value) => value.changeSet.base_tree },
        { header: 'RESULT TREE', value: (value) => value.changeSet.result_tree },
        { header: 'PATCH BYTES', value: (value) => value.changeSet.patch_bytes },
        { header: 'PATCH DIGEST', value: (value) => value.changeSet.patch_digest },
        { header: 'RECORDED AT', value: (value) => value.recordedAt },
      ]
    ),
  };
}

export function agentExecutionMutationResult(row: AgentExecutionMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [{ ...row.execution, replayed: row.replayed }],
      [...EXECUTION_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

export function agentExecutionEventsResult(page: AgentExecutionEventsPage): CommandResult {
  const table = renderTable(page.records, EVENT_COLUMNS);
  return {
    json: page,
    table: page.nextCursor ? `${table}Next cursor: ${page.nextCursor}\n` : table,
  };
}

const EVENT_COLUMNS: readonly TableColumn<AgentExecutionEvent>[] = [
  { header: 'SEQUENCE', value: (row) => row.sequence },
  { header: 'EXECUTION', value: (row) => row.executionId },
  { header: 'KIND', value: (row) => row.kind },
  { header: 'SIZE', value: (row) => row.contentSizeBytes },
  { header: 'OCCURRED AT', value: (row) => row.occurredAt },
];

function listResult<T>(rows: T[], columns: readonly TableColumn<T>[]): CommandResult {
  return { json: rows, table: renderTable(rows, columns) };
}

function singleResult<T>(row: T, columns: readonly TableColumn<T>[]): CommandResult {
  return { json: row, table: renderTable([row], columns) };
}
