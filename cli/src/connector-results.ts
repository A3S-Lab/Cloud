import type {
  ConnectorProfile,
  ConnectorProfileMutationResult,
  ConnectorProfileRecord,
  ConnectorRevision,
} from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

const CONNECTOR_PROFILE_COLUMNS = [
  { header: 'NAME', value: (row: ConnectorProfile) => row.name },
  { header: 'PROFILE', value: (row: ConnectorProfile) => row.profileId },
  { header: 'REVISION', value: (row: ConnectorProfile) => row.currentRevisionNumber },
  { header: 'DIGEST', value: (row: ConnectorProfile) => row.currentRevisionDigest },
  { header: 'VERSION', value: (row: ConnectorProfile) => row.aggregateVersion },
  { header: 'UPDATED AT', value: (row: ConnectorProfile) => row.updatedAt },
] as const;

const CONNECTOR_REVISION_COLUMNS = [
  { header: 'PROFILE', value: (row: ConnectorRevision) => row.profileId },
  { header: 'NUMBER', value: (row: ConnectorRevision) => row.revisionNumber },
  { header: 'REVISION', value: (row: ConnectorRevision) => row.revisionId },
  { header: 'DIGEST', value: (row: ConnectorRevision) => row.definitionDigest },
  { header: 'PARENT', value: (row: ConnectorRevision) => row.parentRevisionId ?? '' },
  { header: 'CREATED AT', value: (row: ConnectorRevision) => row.createdAt },
] as const;

export function connectorProfilesResult(rows: ConnectorProfile[]): CommandResult {
  return { json: rows, table: renderTable(rows, CONNECTOR_PROFILE_COLUMNS) };
}

export function connectorProfileRecordResult(record: ConnectorProfileRecord): CommandResult {
  return {
    json: record,
    table: `${renderTable([record.profile], CONNECTOR_PROFILE_COLUMNS)}${renderTable(
      [record.revision],
      CONNECTOR_REVISION_COLUMNS
    )}`,
  };
}

export function connectorRevisionsResult(rows: ConnectorRevision[]): CommandResult {
  return { json: rows, table: renderTable(rows, CONNECTOR_REVISION_COLUMNS) };
}

export function connectorRevisionResult(row: ConnectorRevision): CommandResult {
  return { json: row, table: renderTable([row], CONNECTOR_REVISION_COLUMNS) };
}

export function connectorProfileMutationResult(result: ConnectorProfileMutationResult): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.record.profile, replayed: result.replayed }],
      [...CONNECTOR_PROFILE_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}
