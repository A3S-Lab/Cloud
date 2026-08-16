import type {
  DurableCellApplication,
  DurableCellApplicationMutationResult,
  DurableCellApplicationRecord,
  DurableCellApplicationRevision,
  DurableCellDeploymentResult,
  DurableCellRoutePublicationResult,
} from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

const APPLICATION_COLUMNS = [
  { header: 'NAME', value: (row: DurableCellApplication) => row.name },
  { header: 'APPLICATION', value: (row: DurableCellApplication) => row.applicationId },
  { header: 'STATE', value: (row: DurableCellApplication) => row.desiredState },
  { header: 'REVISION', value: (row: DurableCellApplication) => row.currentRevisionNumber },
  { header: 'DIGEST', value: (row: DurableCellApplication) => row.currentDefinitionDigest },
  { header: 'VERSION', value: (row: DurableCellApplication) => row.aggregateVersion },
  { header: 'UPDATED AT', value: (row: DurableCellApplication) => row.updatedAt },
] as const;

const REVISION_COLUMNS = [
  { header: 'APPLICATION', value: (row: DurableCellApplicationRevision) => row.applicationId },
  { header: 'NUMBER', value: (row: DurableCellApplicationRevision) => row.revisionNumber },
  { header: 'REVISION', value: (row: DurableCellApplicationRevision) => row.revisionId },
  { header: 'DIGEST', value: (row: DurableCellApplicationRevision) => row.definitionDigest },
  { header: 'PARENT', value: (row: DurableCellApplicationRevision) => row.parentRevisionId ?? '' },
  { header: 'CREATED AT', value: (row: DurableCellApplicationRevision) => row.createdAt },
] as const;

export function durableCellApplicationsResult(rows: DurableCellApplication[]): CommandResult {
  return { json: rows, table: renderTable(rows, APPLICATION_COLUMNS) };
}

export function durableCellApplicationRecordResult(record: DurableCellApplicationRecord): CommandResult {
  return {
    json: record,
    table: `${renderTable([record.application], APPLICATION_COLUMNS)}${renderTable(
      [record.revision],
      REVISION_COLUMNS
    )}`,
  };
}

export function durableCellApplicationRevisionsResult(rows: DurableCellApplicationRevision[]): CommandResult {
  return { json: rows, table: renderTable(rows, REVISION_COLUMNS) };
}

export function durableCellApplicationRevisionResult(row: DurableCellApplicationRevision): CommandResult {
  return { json: row, table: renderTable([row], REVISION_COLUMNS) };
}

export function durableCellApplicationMutationResult(
  result: DurableCellApplicationMutationResult
): CommandResult {
  return {
    json: result,
    table: renderTable(
      [{ ...result.record.application, replayed: result.replayed }],
      [...APPLICATION_COLUMNS, { header: 'REPLAYED', value: (row) => row.replayed }]
    ),
  };
}

export function durableCellDeploymentResult(result: DurableCellDeploymentResult): CommandResult {
  const row = {
    applicationId: result.correlation.applicationId,
    revisionNumber: result.correlation.applicationRevisionNumber,
    workloadId: result.correlation.workloadId,
    deploymentId: result.correlation.deploymentId,
    operationId: result.correlation.operationId,
    providerArtifactDigest: result.correlation.providerArtifactDigest,
    replayed: result.replayed,
  };
  return {
    json: result,
    table: renderTable(
      [row],
      [
        { header: 'APPLICATION', value: (value) => value.applicationId },
        { header: 'REVISION', value: (value) => value.revisionNumber },
        { header: 'WORKLOAD', value: (value) => value.workloadId },
        { header: 'DEPLOYMENT', value: (value) => value.deploymentId },
        { header: 'OPERATION', value: (value) => value.operationId },
        { header: 'ARTIFACT', value: (value) => value.providerArtifactDigest },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

export function durableCellRoutePublicationResult(result: DurableCellRoutePublicationResult): CommandResult {
  const row = {
    applicationId: result.correlation.applicationId,
    revisionNumber: result.correlation.applicationRevisionNumber,
    routeId: result.publication.route.id,
    hostname: result.publication.route.hostname,
    pathPrefix: result.publication.route.pathPrefix,
    state: result.publication.route.state,
    replayed: result.publication.replayed,
  };
  return {
    json: result,
    table: renderTable(
      [row],
      [
        { header: 'APPLICATION', value: (value) => value.applicationId },
        { header: 'REVISION', value: (value) => value.revisionNumber },
        { header: 'ROUTE', value: (value) => value.routeId },
        { header: 'HOSTNAME', value: (value) => value.hostname },
        { header: 'PATH', value: (value) => value.pathPrefix },
        { header: 'STATE', value: (value) => value.state },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}
