import type { EnrollmentToken } from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

export interface NodeBootstrapResult extends EnrollmentToken {
  installationInvocation: string;
}

const ENROLLMENT_TOKEN_COLUMNS: readonly TableColumn<NodeBootstrapResult>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'NAME', value: (row) => row.name },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'EXPIRES AT', value: (row) => row.expiresAt },
  { header: 'USED AT', value: (row) => row.usedAt ?? '' },
  { header: 'REPLAYED', value: (row) => row.replayed },
];

export function nodeBootstrapResult(
  row: EnrollmentToken,
  installationInvocation: string,
  enrollmentCredential: string,
  requestedName: string
): CommandResult {
  const hide = (value: string): string => value.split(enrollmentCredential).join('[redacted]');
  const safeRow: NodeBootstrapResult = {
    id: hide(row.id),
    organizationId: hide(row.organizationId),
    name: requestedName,
    aggregateVersion: row.aggregateVersion,
    createdAt: hide(row.createdAt),
    expiresAt: hide(row.expiresAt),
    usedAt: row.usedAt === null ? null : hide(row.usedAt),
    revokedAt: row.revokedAt === null ? null : hide(row.revokedAt),
    replayed: row.replayed,
    installationInvocation: hide(installationInvocation),
  };
  return {
    json: safeRow,
    table:
      `${renderTable([safeRow], ENROLLMENT_TOKEN_COLUMNS)}` +
      `\nInstallation invocation (Bash):\n${safeRow.installationInvocation}\n`,
  };
}
