import {
  DEFAULT_SECURITY_TIMELINE_LIMIT,
  type CloudApi,
  encodeSecurityTimelineQuery,
  MAX_SECURITY_TIMELINE_LIMIT,
  type SecurityTimelineQuery,
} from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  requireArity,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization } from './context';
import { inputValidationUsageError, usageError } from './errors';
import type { CommandResult } from './results';
import { gatewayRoutePolicyTimelineResult } from './security-results';

const SECURITY_TIMELINE_COMMAND = 'security-investigations timeline';

export async function executeSecurityCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi
): Promise<CommandResult | undefined> {
  if (command !== SECURITY_TIMELINE_COMMAND) {
    return undefined;
  }
  requireArity(arguments_.positionals, 3, `${SECURITY_TIMELINE_COMMAND} ROUTE_ID`);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  if (arguments_.stream !== undefined) {
    throw usageError('--stream is valid only for log commands');
  }
  const routeId = positionalUuid(arguments_.positionals, 2, 'Gateway Route policy route ID');
  const query: SecurityTimelineQuery = {
    cursor: arguments_.cursor,
    limit: securityTimelineLimit(arguments_.limit),
  };
  try {
    encodeSecurityTimelineQuery(query);
  } catch (error) {
    throw inputValidationUsageError(error);
  }
  return gatewayRoutePolicyTimelineResult(
    await cloudApi().listGatewayRoutePolicySecurityTimeline(requireOrganization(context), routeId, query)
  );
}

function securityTimelineLimit(value: string | undefined): number {
  if (value === undefined) {
    return DEFAULT_SECURITY_TIMELINE_LIMIT;
  }
  if (!/^[0-9]+$/u.test(value)) {
    throw usageError('security timeline limit must be an integer');
  }
  const limit = Number(value);
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_SECURITY_TIMELINE_LIMIT) {
    throw usageError(`security timeline limit must be between 1 and ${MAX_SECURITY_TIMELINE_LIMIT}`);
  }
  return limit;
}
