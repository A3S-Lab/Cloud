import {
  DEFAULT_AUDIT_EXPORT_MANIFEST_PAGE_SIZE,
  DEFAULT_AUDIT_RECORD_LIMIT,
  MAX_AUDIT_RECORD_LIMIT,
  type AuditAttributionStatus,
  type AuditExportManifestQuery,
  type AuditExportQuery,
  type AuditRecordQuery,
  type CloudApi,
  encodeAuditExportManifestQuery,
  encodeAuditExportQuery,
  encodeAuditRecordQuery,
} from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  requireArity,
} from './command-options';
import type { CloudContext } from './context';
import { parseUuid, requireOrganization } from './context';
import { inputValidationUsageError, usageError } from './errors';
import type { CommandResult } from './results';
import {
  auditExportManifestResult,
  auditExportResult,
  auditRecordsResult,
  auditRetentionResult,
} from './audit-results';

const AUDIT_LIST_COMMAND = 'audit-records list';
const AUDIT_EXPORT_COMMAND = 'audit-records export';
const AUDIT_EXPORT_MANIFEST_COMMAND = 'audit-records export-manifest';
const AUDIT_RETENTION_COMMAND = 'audit-records retention';

export async function executeAuditCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi
): Promise<CommandResult | undefined> {
  if (
    command !== AUDIT_LIST_COMMAND &&
    command !== AUDIT_EXPORT_COMMAND &&
    command !== AUDIT_EXPORT_MANIFEST_COMMAND &&
    command !== AUDIT_RETENTION_COMMAND
  ) {
    return undefined;
  }
  requireArity(arguments_.positionals, 2, command);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  if (arguments_.stream !== undefined) {
    throw usageError('--stream is valid only for log commands');
  }
  if (command === AUDIT_RETENTION_COMMAND) {
    rejectAuditQueryOptions(arguments_);
    return auditRetentionResult(await cloudApi().getAuditRetentionStatus(requireOrganization(context)));
  }

  const selection = {
    actorPrincipalId: optionalUuid(arguments_.auditActorPrincipalId, 'audit actor Principal ID'),
    action: arguments_.auditAction,
    aggregateId: optionalUuid(arguments_.auditAggregateId, 'audit aggregate ID'),
    requestId: optionalUuid(arguments_.auditRequestId, 'audit request ID'),
    projectId: optionalUuid(arguments_.projectId, 'audit Project ID'),
    environmentId: optionalUuid(arguments_.environmentId, 'audit Environment ID'),
    attributionProfileId: optionalUuid(arguments_.auditAttributionProfileId, 'audit attribution profile ID'),
    attributionStatus: arguments_.auditAttributionStatus as AuditAttributionStatus | undefined,
  };
  if (command === AUDIT_EXPORT_MANIFEST_COMMAND) {
    if (arguments_.cursor !== undefined) {
      throw usageError('--cursor is not valid for audit-records export-manifest');
    }
    const query: AuditExportManifestQuery = {
      ...selection,
      from: requireAuditTimestamp(arguments_.auditFrom, '--from', command),
      to: requireAuditTimestamp(arguments_.auditTo, '--to', command),
      pageSize: auditManifestPageSize(arguments_.limit),
    };
    try {
      encodeAuditExportManifestQuery(query);
    } catch (error) {
      throw inputValidationUsageError(error);
    }
    return auditExportManifestResult(
      await cloudApi().exportAuditRecordManifest(requireOrganization(context), query)
    );
  }
  if (command === AUDIT_EXPORT_COMMAND) {
    const query: AuditExportQuery = {
      ...selection,
      cursor: arguments_.cursor,
      limit: auditLimit(arguments_.limit),
      from: requireAuditTimestamp(arguments_.auditFrom, '--from', command),
      to: requireAuditTimestamp(arguments_.auditTo, '--to', command),
    };
    try {
      encodeAuditExportQuery(query);
    } catch (error) {
      throw inputValidationUsageError(error);
    }
    return auditExportResult(await cloudApi().exportAuditRecords(requireOrganization(context), query));
  }
  const query: AuditRecordQuery = {
    ...selection,
    from: arguments_.auditFrom,
    to: arguments_.auditTo,
    cursor: arguments_.cursor,
    limit: auditLimit(arguments_.limit),
  };
  try {
    encodeAuditRecordQuery(query);
  } catch (error) {
    throw inputValidationUsageError(error);
  }
  return auditRecordsResult(await cloudApi().listAuditRecords(requireOrganization(context), query));
}

export function rejectMisplacedAuditOptions(command: string, arguments_: ParsedArguments): void {
  if (
    command === AUDIT_LIST_COMMAND ||
    command === AUDIT_EXPORT_COMMAND ||
    command === AUDIT_EXPORT_MANIFEST_COMMAND ||
    command === AUDIT_RETENTION_COMMAND
  ) {
    return;
  }
  if (
    arguments_.auditActorPrincipalId !== undefined ||
    arguments_.auditAction !== undefined ||
    arguments_.auditAggregateId !== undefined ||
    arguments_.auditRequestId !== undefined ||
    arguments_.auditAttributionProfileId !== undefined ||
    arguments_.auditAttributionStatus !== undefined ||
    arguments_.auditFrom !== undefined ||
    arguments_.auditTo !== undefined
  ) {
    throw usageError(
      '--actor-principal, --action, --aggregate, --request-id, --attribution-profile, --attribution-status, --from, and --to are valid only for audit-records list, export, or export-manifest'
    );
  }
}

function rejectAuditQueryOptions(arguments_: ParsedArguments): void {
  if (
    arguments_.auditActorPrincipalId !== undefined ||
    arguments_.auditAction !== undefined ||
    arguments_.auditAggregateId !== undefined ||
    arguments_.auditRequestId !== undefined ||
    arguments_.auditAttributionProfileId !== undefined ||
    arguments_.auditAttributionStatus !== undefined ||
    arguments_.auditFrom !== undefined ||
    arguments_.auditTo !== undefined ||
    arguments_.projectId !== undefined ||
    arguments_.environmentId !== undefined ||
    arguments_.cursor !== undefined ||
    arguments_.limit !== undefined
  ) {
    throw usageError('audit-records retention does not accept record query options');
  }
}

function requireAuditTimestamp(value: string | undefined, option: string, command: string): string {
  if (value === undefined) {
    throw usageError(`${option} is required for ${command}`);
  }
  return value;
}

function optionalUuid(value: string | undefined, label: string): string | undefined {
  return value === undefined ? undefined : parseUuid(value, label);
}

function auditLimit(value: string | undefined): number {
  if (value === undefined) {
    return DEFAULT_AUDIT_RECORD_LIMIT;
  }
  if (!/^[0-9]+$/u.test(value)) {
    throw usageError('audit record limit must be an integer');
  }
  const limit = Number(value);
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_AUDIT_RECORD_LIMIT) {
    throw usageError(`audit record limit must be between 1 and ${MAX_AUDIT_RECORD_LIMIT}`);
  }
  return limit;
}

function auditManifestPageSize(value: string | undefined): number {
  if (value === undefined) {
    return DEFAULT_AUDIT_EXPORT_MANIFEST_PAGE_SIZE;
  }
  if (!/^[0-9]+$/u.test(value)) {
    throw usageError('audit export manifest page size must be an integer');
  }
  const pageSize = Number(value);
  if (!Number.isSafeInteger(pageSize) || pageSize < 1 || pageSize > MAX_AUDIT_RECORD_LIMIT) {
    throw usageError(`audit export manifest page size must be between 1 and ${MAX_AUDIT_RECORD_LIMIT}`);
  }
  return pageSize;
}
