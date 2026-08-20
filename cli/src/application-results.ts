import type { Application, ApplicationMutationResult, ApplicationRelease } from '@a3s/cloud-client';
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
