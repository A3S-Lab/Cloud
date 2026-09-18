import type {
  ApiToken,
  ApiTokenMutationResult,
  DirectoryMembershipProjectionBinding,
  DirectoryMembershipProjectionMutationResult,
  DirectoryResourceGrant,
  DirectoryResourceGrantMutationResult,
  Membership,
  MembershipInvitation,
  MembershipInvitationAcceptanceResult,
  MembershipInvitationMutationResult,
  MembershipMutationResult,
  PartnerSubjectLink,
  PartnerSubjectLinkMutationResult,
  ResourceGrant,
  ResourceGrantMutationResult,
} from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const API_TOKEN_COLUMNS: readonly TableColumn<ApiToken>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'NAME', value: (row) => row.name },
  { header: 'PRINCIPAL', value: (row) => row.principalId },
  { header: 'SCOPES', value: (row) => row.scopes.join(',') },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'EXPIRES AT', value: (row) => row.expiresAt ?? '' },
  { header: 'REVOKED AT', value: (row) => row.revokedAt ?? '' },
];

export function apiTokensResult(rows: ApiToken[]): CommandResult {
  const safeRows = rows.map(safeApiToken);
  return { json: safeRows, table: renderTable(safeRows, API_TOKEN_COLUMNS) };
}

export function apiTokenResult(row: ApiToken): CommandResult {
  const safeRow = safeApiToken(row);
  return { json: safeRow, table: renderTable([safeRow], API_TOKEN_COLUMNS) };
}

export function apiTokenMutationResult(row: ApiTokenMutationResult): CommandResult {
  const safeRow = { ...safeApiToken(row), replayed: row.replayed };
  return {
    json: safeRow,
    table: renderTable(
      [safeRow],
      [...API_TOKEN_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

function safeApiToken(row: ApiToken): ApiToken {
  return {
    id: row.id,
    organizationId: row.organizationId,
    principalId: row.principalId,
    name: row.name,
    scopes: [...row.scopes],
    aggregateVersion: row.aggregateVersion,
    createdAt: row.createdAt,
    expiresAt: row.expiresAt,
    revokedAt: row.revokedAt,
  };
}

const MEMBERSHIP_COLUMNS: readonly TableColumn<Membership>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'PRINCIPAL', value: (row) => row.principalId },
  { header: 'NAME', value: (row) => row.principalName },
  { header: 'KIND', value: (row) => row.principalKind },
  { header: 'ROLE', value: (row) => row.role },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'REVOKED AT', value: (row) => row.revokedAt ?? '' },
];

export function membershipsResult(rows: Membership[]): CommandResult {
  return { json: rows, table: renderTable(rows, MEMBERSHIP_COLUMNS) };
}

export function membershipResult(row: Membership): CommandResult {
  return { json: row, table: renderTable([row], MEMBERSHIP_COLUMNS) };
}

export function membershipMutationResult(row: MembershipMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [...MEMBERSHIP_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

const MEMBERSHIP_INVITATION_COLUMNS: readonly TableColumn<MembershipInvitation>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'ORGANIZATION', value: (row) => row.organizationId },
  { header: 'PRINCIPAL', value: (row) => row.principalId },
  { header: 'ROLE', value: (row) => row.role },
  { header: 'STATUS', value: (row) => row.status },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'EXPIRES AT', value: (row) => row.expiresAt },
];

export function membershipInvitationsResult(rows: MembershipInvitation[]): CommandResult {
  return { json: rows, table: renderTable(rows, MEMBERSHIP_INVITATION_COLUMNS) };
}

export function membershipInvitationResult(row: MembershipInvitation): CommandResult {
  return { json: row, table: renderTable([row], MEMBERSHIP_INVITATION_COLUMNS) };
}

export function membershipInvitationMutationResult(row: MembershipInvitationMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [...MEMBERSHIP_INVITATION_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

export function membershipInvitationAcceptanceResult(
  row: MembershipInvitationAcceptanceResult
): CommandResult {
  return {
    json: row,
    table: renderTable(
      [{ ...row.invitation, replayed: row.replayed }],
      [...MEMBERSHIP_INVITATION_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

const RESOURCE_GRANT_COLUMNS: readonly TableColumn<ResourceGrant>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'MEMBERSHIP', value: (row) => row.membershipId },
  { header: 'KIND', value: (row) => row.scope.kind },
  { header: 'RESOURCE', value: resourceGrantScopeIdentity },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'REVOKED AT', value: (row) => row.revokedAt ?? '' },
];

export function resourceGrantsResult(rows: ResourceGrant[]): CommandResult {
  return { json: rows, table: renderTable(rows, RESOURCE_GRANT_COLUMNS) };
}

export function resourceGrantResult(row: ResourceGrant): CommandResult {
  return { json: row, table: renderTable([row], RESOURCE_GRANT_COLUMNS) };
}

export function resourceGrantMutationResult(row: ResourceGrantMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [...RESOURCE_GRANT_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

function resourceGrantScopeIdentity(row: ResourceGrant | DirectoryResourceGrant): string {
  switch (row.scope.kind) {
    case 'project':
      return row.scope.projectId;
    case 'environment':
      return `${row.scope.projectId}/${row.scope.environmentId}`;
    case 'application':
      return `${row.scope.projectId}/${row.scope.applicationId}`;
    case 'node':
      return row.scope.nodeId;
  }
}

const PARTNER_SUBJECT_LINK_COLUMNS: readonly TableColumn<PartnerSubjectLink>[] = [
  { header: 'LINK', value: (row) => row.linkId },
  { header: 'PROVIDER', value: (row) => row.providerKey },
  { header: 'ISSUER', value: (row) => row.issuer },
  { header: 'SUBJECT', value: (row) => row.subject },
  { header: 'PRINCIPAL', value: (row) => row.principalId },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'REVOKED AT', value: (row) => row.revokedAt ?? '' },
];

export function partnerSubjectLinksResult(rows: PartnerSubjectLink[]): CommandResult {
  return { json: rows, table: renderTable(rows, PARTNER_SUBJECT_LINK_COLUMNS) };
}

export function partnerSubjectLinkResult(row: PartnerSubjectLink): CommandResult {
  return { json: row, table: renderTable([row], PARTNER_SUBJECT_LINK_COLUMNS) };
}

export function partnerSubjectLinkMutationResult(row: PartnerSubjectLinkMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [...PARTNER_SUBJECT_LINK_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

const DIRECTORY_RESOURCE_GRANT_COLUMNS: readonly TableColumn<DirectoryResourceGrant>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'SUBJECT', value: (row) => row.subjectRef },
  { header: 'KIND', value: (row) => row.scope.kind },
  { header: 'RESOURCE', value: resourceGrantScopeIdentity },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'REVOKED AT', value: (row) => row.revokedAt ?? '' },
];

export function directoryResourceGrantsResult(rows: DirectoryResourceGrant[]): CommandResult {
  return { json: rows, table: renderTable(rows, DIRECTORY_RESOURCE_GRANT_COLUMNS) };
}

export function directoryResourceGrantResult(row: DirectoryResourceGrant): CommandResult {
  return { json: row, table: renderTable([row], DIRECTORY_RESOURCE_GRANT_COLUMNS) };
}

export function directoryResourceGrantMutationResult(
  row: DirectoryResourceGrantMutationResult
): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [...DIRECTORY_RESOURCE_GRANT_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

const DIRECTORY_MEMBERSHIP_PROJECTION_COLUMNS: readonly TableColumn<DirectoryMembershipProjectionBinding>[] =
  [
    { header: 'SUBJECT', value: (row) => row.subjectRef },
    { header: 'PRINCIPAL', value: (row) => row.principalId },
    { header: 'CREATED AT', value: (row) => row.createdAt },
  ];

export function directoryMembershipProjectionsResult(
  rows: DirectoryMembershipProjectionBinding[]
): CommandResult {
  return { json: rows, table: renderTable(rows, DIRECTORY_MEMBERSHIP_PROJECTION_COLUMNS) };
}

export function directoryMembershipProjectionMutationResult(
  row: DirectoryMembershipProjectionMutationResult
): CommandResult {
  return {
    json: row,
    table: renderTable(
      row.items.map((item) => ({ ...item, replayed: row.replayed })),
      [
        ...DIRECTORY_MEMBERSHIP_PROJECTION_COLUMNS,
        { header: 'REPLAYED', value: (value: { replayed: boolean }) => value.replayed },
      ]
    ),
  };
}
