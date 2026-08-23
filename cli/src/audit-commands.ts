import {
  DEFAULT_AUDIT_RECORD_LIMIT,
  MAX_AUDIT_RECORD_LIMIT,
  type AuditAttributionStatus,
  type AuditRecordQuery,
  type CloudApi,
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
import { auditRecordsResult } from './audit-results';

const AUDIT_COMMAND = 'audit-records list';

export async function executeAuditCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi
): Promise<CommandResult | undefined> {
  if (command !== AUDIT_COMMAND) {
    return undefined;
  }
  requireArity(arguments_.positionals, 2, AUDIT_COMMAND);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  if (arguments_.stream !== undefined) {
    throw usageError('--stream is valid only for log commands');
  }

  const query: AuditRecordQuery = {
    actorPrincipalId: optionalUuid(arguments_.auditActorPrincipalId, 'audit actor Principal ID'),
    action: arguments_.auditAction,
    aggregateId: optionalUuid(arguments_.auditAggregateId, 'audit aggregate ID'),
    requestId: optionalUuid(arguments_.auditRequestId, 'audit request ID'),
    projectId: optionalUuid(arguments_.projectId, 'audit Project ID'),
    environmentId: optionalUuid(arguments_.environmentId, 'audit Environment ID'),
    attributionProfileId: optionalUuid(arguments_.auditAttributionProfileId, 'audit attribution profile ID'),
    attributionStatus: arguments_.auditAttributionStatus as AuditAttributionStatus | undefined,
    from: arguments_.auditFrom,
    to: arguments_.auditTo,
    cursor: arguments_.cursor,
    limit: auditLimit(arguments_.limit),
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
  if (command === AUDIT_COMMAND) {
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
      '--actor-principal, --action, --aggregate, --request-id, --attribution-profile, --attribution-status, --from, and --to are valid only for audit-records list'
    );
  }
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
