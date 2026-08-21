import type {
  Application,
  ApplicationInvocation,
  ApplicationInvocationCancellationResult,
  ApplicationInvocationMutationResult,
  ApplicationMessage,
  ApplicationMutationResult,
  ApplicationRelease,
  ApplicationSession,
  ApplicationSessionMutationResult,
  ApplicationSessionReplay,
} from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

const APPLICATION_COLUMNS = [
  { header: 'NAME', value: (row: Application) => row.name },
  { header: 'APPLICATION', value: (row: Application) => row.applicationId },
  { header: 'EXPERIENCE', value: (row: Application) => row.experience },
  { header: 'RELEASE', value: (row: Application) => row.currentReleaseNumber },
  { header: 'DIGEST', value: (row: Application) => row.currentReleaseDigest },
  { header: 'VERSION', value: (row: Application) => row.aggregateVersion },
  { header: 'UPDATED AT', value: (row: Application) => row.updatedAt },
] as const;

const APPLICATION_RELEASE_COLUMNS = [
  { header: 'APPLICATION', value: (row: ApplicationRelease) => row.applicationId },
  { header: 'NUMBER', value: (row: ApplicationRelease) => row.releaseNumber },
  { header: 'RELEASE', value: (row: ApplicationRelease) => row.releaseId },
  { header: 'EXPERIENCE', value: (row: ApplicationRelease) => row.experience },
  { header: 'DIGEST', value: (row: ApplicationRelease) => row.contractDigest },
  { header: 'PARENT', value: (row: ApplicationRelease) => row.parentReleaseId ?? '' },
  { header: 'CREATED AT', value: (row: ApplicationRelease) => row.createdAt },
] as const;

const APPLICATION_SESSION_COLUMNS = [
  { header: 'SESSION', value: (row: ApplicationSession) => row.sessionId },
  { header: 'APPLICATION', value: (row: ApplicationSession) => row.applicationId },
  { header: 'RELEASE', value: (row: ApplicationSession) => row.applicationReleaseNumber },
  { header: 'MODE', value: (row: ApplicationSession) => row.interactionMode },
  { header: 'STATUS', value: (row: ApplicationSession) => row.status },
  { header: 'MESSAGES', value: (row: ApplicationSession) => row.lastMessageSequence },
  { header: 'VERSION', value: (row: ApplicationSession) => row.aggregateVersion },
] as const;

const APPLICATION_INVOCATION_COLUMNS = [
  { header: 'INVOCATION', value: (row: ApplicationInvocation) => row.invocationId },
  { header: 'SESSION', value: (row: ApplicationInvocation) => row.sessionId },
  { header: 'MODE', value: (row: ApplicationInvocation) => row.responseMode },
  { header: 'STATUS', value: (row: ApplicationInvocation) => row.status },
  { header: 'WORKFLOW RUN', value: (row: ApplicationInvocation) => row.workflowRunId ?? '' },
  { header: 'REQUESTED AT', value: (row: ApplicationInvocation) => row.requestedAt },
] as const;

const APPLICATION_MESSAGE_COLUMNS = [
  { header: 'SEQUENCE', value: (row: ApplicationMessage) => row.sequence },
  { header: 'KIND', value: (row: ApplicationMessage) => row.kind },
  { header: 'INVOCATION', value: (row: ApplicationMessage) => row.invocationId },
  { header: 'MESSAGE', value: (row: ApplicationMessage) => row.messageId },
  { header: 'DIGEST', value: (row: ApplicationMessage) => row.contentDigest },
  { header: 'CREATED AT', value: (row: ApplicationMessage) => row.createdAt },
] as const;

export function applicationsResult(rows: Application[]): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_COLUMNS) };
}

export function applicationResult(row: Application): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_COLUMNS) };
}

export function applicationReleasesResult(rows: ApplicationRelease[]): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_RELEASE_COLUMNS) };
}

export function applicationReleaseResult(row: ApplicationRelease): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_RELEASE_COLUMNS) };
}

export function applicationMutationResult(result: ApplicationMutationResult): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.record.application, replayed: result.replayed }],
      [...APPLICATION_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}

export function applicationSessionResult(row: ApplicationSession): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_SESSION_COLUMNS) };
}

export function applicationSessionMutationResult(result: ApplicationSessionMutationResult): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.session, replayed: result.replayed }],
      [...APPLICATION_SESSION_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}

export function applicationInvocationResult(row: ApplicationInvocation): CommandResult {
  return { json: row, table: renderTable([row], APPLICATION_INVOCATION_COLUMNS) };
}

export function applicationInvocationMutationResult(
  result: ApplicationInvocationMutationResult
): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.invocation, replayed: result.replayed }],
      [...APPLICATION_INVOCATION_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}

export function applicationInvocationCancellationResult(
  result: ApplicationInvocationCancellationResult
): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.invocation, replayed: result.replayed }],
      [...APPLICATION_INVOCATION_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}

export function applicationMessagesResult(rows: ApplicationMessage[]): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_MESSAGE_COLUMNS) };
}

export function applicationSessionReplayResult(result: ApplicationSessionReplay): CommandResult {
  return {
    json: result,
    table: renderTable(result.messages, APPLICATION_MESSAGE_COLUMNS),
  };
}
