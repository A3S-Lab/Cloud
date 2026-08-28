import type { UserFile, UserFileMutationResult, UserFileQuota } from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const USER_FILE_COLUMNS: readonly TableColumn<UserFile>[] = [
  { header: 'ID', value: (row) => row.userFileId },
  { header: 'STATE', value: (row) => row.state },
  { header: 'NAME', value: (row) => row.originalName },
  { header: 'SIZE', value: (row) => row.sizeBytes },
  { header: 'MEDIA TYPE', value: (row) => row.mediaType },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'UPLOAD EXPIRES', value: (row) => row.uploadExpiresAt },
  { header: 'RETENTION UNTIL', value: (row) => row.retentionUntil },
  { header: 'CLEANUP DUE', value: (row) => row.cleanupDueAt },
];

export function userFilesResult(rows: UserFile[]): CommandResult {
  return {
    json: rows,
    table: renderTable(rows, USER_FILE_COLUMNS),
  };
}

export function userFileResult(row: UserFile): CommandResult {
  return {
    json: row,
    table: renderTable([row], USER_FILE_COLUMNS),
  };
}

export function userFileMutationResult(result: UserFileMutationResult): CommandResult {
  const row = { ...result.file, replayed: result.replayed };
  return {
    json: result,
    table: renderTable(
      [row],
      [...USER_FILE_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

export function userFileQuotaResult(row: UserFileQuota): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ORGANIZATION', value: (value) => value.organizationId },
        { header: 'LIMIT BYTES', value: (value) => value.limitBytes },
        { header: 'ALLOCATED BYTES', value: (value) => value.allocatedBytes },
        { header: 'AVAILABLE BYTES', value: (value) => value.availableBytes },
        { header: 'REVISION', value: (value) => value.revision },
        { header: 'UPDATED AT', value: (value) => value.updatedAt },
      ]
    ),
  };
}
