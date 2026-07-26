import {
  DEFAULT_SEARCH_LIMIT,
  MAX_SEARCH_RESULTS,
  type CloudApi,
  validateSearchRequest,
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
import { requireOrganization } from './context';
import { usageError } from './errors';
import type { CommandResult } from './results';
import { searchResultsResult } from './search-results';

export async function executeSearchCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi
): Promise<CommandResult | undefined> {
  if (command !== 'search resources') {
    return undefined;
  }

  requireArity(arguments_.positionals, 3, 'search resources <query>');
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  rejectLogOnlyOptions(arguments_);

  const limit = searchLimit(arguments_.limit);
  const query = searchQuery(arguments_.positionals[2], limit);
  const organizationId = requireOrganization(context);
  const api = cloudApi();
  return searchResultsResult(await api.searchResources(organizationId, query, limit));
}

function rejectLogOnlyOptions(arguments_: ParsedArguments): void {
  if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
    throw usageError('cursor and stream options are valid only for log commands');
  }
}

function searchLimit(value: string | undefined): number {
  if (value === undefined) {
    return DEFAULT_SEARCH_LIMIT;
  }
  if (!/^[0-9]+$/.test(value)) {
    throw usageError('search result limit must be an integer');
  }
  const limit = Number(value);
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_SEARCH_RESULTS) {
    throw usageError(`search result limit must be between 1 and ${MAX_SEARCH_RESULTS}`);
  }
  return limit;
}

function searchQuery(value: string, limit: number): string {
  try {
    return validateSearchRequest(value, limit);
  } catch (error) {
    if (error instanceof RangeError) {
      throw usageError(error.message);
    }
    throw error;
  }
}
