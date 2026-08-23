import {
  DEFAULT_AUDIT_RECORD_LIMIT,
  MAX_AUDIT_RECORD_LIMIT,
  type AuditAttributionStatus,
  type AuditExportQuery,
  type AuditRecordQuery,
  type CloudApi,
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
import { usageError } from './errors';
import type { CommandResult } from './results';
import { auditExportResult, auditRecordsResult } from './audit-results';

const AUDIT_LIST_COMMAND = 'audit-records list';
const AUDIT_EXPORT_COMMAND = 'audit-records export';

export async function executeAuditCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi
): Promise<CommandResult | undefined> {
  if (command !== AUDIT_LIST_COMMAND && command !== AUDIT_EXPORT_COMMAND) {
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

  const selection = {
    actorPrincipalId: optionalUuid(arguments_.auditActorPrincipalId, 'audit actor Principal ID'),
    action: arguments_.auditAction,
    aggregateId: optionalUuid(arguments_.auditAggregateId, 'audit aggregate ID'),
    requestId: optionalUuid(arguments_.auditRequestId, 'audit request ID'),
    projectId: optionalUuid(arguments_.projectId, 'audit Project ID'),
    environmentId: optionalUuid(arguments_.environmentId, 'audit Environment ID'),
    attributionProfileId: optionalUuid(arguments_.auditAttributionProfileId, 'audit attribution profile ID'),
    attributionStatus: arguments_.auditAttributionStatus as AuditAttributionStatus | undefined,
    cursor: arguments_.cursor,
    limit: auditLimit(arguments_.limit),
  };
  if (command === AUDIT_EXPORT_COMMAND) {
    const query: AuditExportQuery = {
      ...selection,
      from: requireAuditTimestamp(arguments_.auditFrom, '--from'),
      to: requireAuditTimestamp(arguments_.auditTo, '--to'),
    };
    try {
      encodeAuditExportQuery(query);
    } catch (error) {
      if (error instanceof TypeError || error instanceof RangeError) {
        throw usageError(error.message);
      }
      throw error;
    }
    return auditExportResult(await cloudApi().exportAuditRecords(requireOrganization(context), query));
  }
  const query: AuditRecordQuery = {
    ...selection,
    from: arguments_.auditFrom,
    to: arguments_.auditTo,
  };
  try {
    encodeAuditRecordQuery(query);
  } catch (error) {
    if (error instanceof TypeError || error instanceof RangeError) {
      throw usageError(error.message);
    }
    throw error;
  }
  return auditRecordsResult(await cloudApi().listAuditRecords(requireOrganization(context), query));
}

export function rejectMisplacedAuditOptions(command: string, arguments_: ParsedArguments): void {
  if (command === AUDIT_LIST_COMMAND || command === AUDIT_EXPORT_COMMAND) {
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
      '--actor-principal, --action, --aggregate, --request-id, --attribution-profile, --attribution-status, --from, and --to are valid only for audit-records list or export'
    );
  }
}

function requireAuditTimestamp(value: string | undefined, option: string): string {
  if (value === undefined) {
    throw usageError(`${option} is required for audit-records export`);
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
