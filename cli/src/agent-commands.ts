import {
  type CloudApi,
  type CloudSequenceQuery,
  validateAgentProviderKind,
} from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  requireArity,
  requireListCommand,
  requireMutationCommand,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { hasUnsafeControl, requireEnvironment, requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import {
  agentConversationMutationResult,
  agentConversationResult,
  agentConversationsResult,
  agentExecutionEventsResult,
  agentExecutionChangeSetResult,
  agentExecutionMutationResult,
  agentExecutionResult,
  agentExecutionsResult,
} from './agent-results';
import type { CommandResult } from './results';

export async function executeAgentCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  const organizationId = () => requireOrganization(context);
  switch (command) {
    case 'agent-conversations list':
      requireListCommand(arguments_);
      return agentConversationsResult(
        await cloudApi().listAgentConversations(
          organizationId(),
          requireProject(context),
          requireEnvironment(context)
        )
      );
    case 'agent-conversations get':
      requireReadCommand(arguments_, 'agent-conversations get <conversation-id>');
      return agentConversationResult(
        await cloudApi().getAgentConversation(
          organizationId(),
          positionalUuid(positionals, 2, 'Agent conversation ID')
        )
      );
    case 'agent-conversations create': {
      const idempotencyKey = requireMutationCommand(arguments_, 2, 'agent-conversations create');
      return agentConversationMutationResult(
        await cloudApi().createAgentConversation(
          organizationId(),
          requireProject(context),
          requireEnvironment(context),
          idempotencyKey
        )
      );
    }
    case 'agent-conversations events': {
      requireAgentEventRead(arguments_);
      return agentExecutionEventsResult(
        await cloudApi().getAgentExecutionEvents(
          organizationId(),
          positionalUuid(positionals, 2, 'Agent conversation ID'),
          parseEventQuery(arguments_)
        )
      );
    }
    case 'agent-executions list':
      requireReadCommand(arguments_, 'agent-executions list <conversation-id>');
      return agentExecutionsResult(
        await cloudApi().listAgentExecutions(
          organizationId(),
          positionalUuid(positionals, 2, 'Agent conversation ID')
        )
      );
    case 'agent-executions get':
      requireReadCommand(arguments_, 'agent-executions get <execution-id>');
      return agentExecutionResult(
        await cloudApi().getAgentExecution(
          organizationId(),
          positionalUuid(positionals, 2, 'Agent execution ID')
        )
      );
    case 'agent-executions changes':
      requireReadCommand(arguments_, 'agent-executions changes <execution-id>');
      return agentExecutionChangeSetResult(
        await cloudApi().getAgentExecutionChangeSet(
          organizationId(),
          positionalUuid(positionals, 2, 'Agent execution ID')
        )
      );
    case 'agent-executions start': {
      const providerKind = arguments_.providerKind;
      validateAgentProviderKind(providerKind);
      const idempotencyKey = requireMutationCommand(
        arguments_,
        5,
        'agent-executions start <conversation-id> <agent-asset-id> <agent-release-id>',
        true
      );
      return agentExecutionMutationResult(
        await cloudApi().startAgentExecution(
          organizationId(),
          positionalUuid(positionals, 2, 'Agent conversation ID'),
          {
            agentAssetId: positionalUuid(positionals, 3, 'Agent Asset ID'),
            agentAssetReleaseId: positionalUuid(positionals, 4, 'Agent Asset release ID'),
            ...(providerKind === undefined ? {} : { providerKind }),
          },
          idempotencyKey
        )
      );
    }
    case 'agent-executions cancel': {
      const idempotencyKey = requireMutationCommand(arguments_, 3, 'agent-executions cancel <execution-id>');
      return agentExecutionMutationResult(
        await cloudApi().cancelAgentExecution(
          organizationId(),
          positionalUuid(positionals, 2, 'Agent execution ID'),
          idempotencyKey
        )
      );
    }
    default:
      return undefined;
  }
}

function requireAgentEventRead(arguments_: ParsedArguments): void {
  requireArity(arguments_.positionals, 3, 'agent-conversations events <conversation-id>');
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  if (arguments_.stream !== undefined) {
    throw usageError('--stream is valid only for log commands');
  }
}

function parseEventQuery(arguments_: ParsedArguments): CloudSequenceQuery {
  const query: CloudSequenceQuery = {};
  if (arguments_.cursor !== undefined) {
    const cursor = arguments_.cursor;
    if (cursor.length === 0 || cursor.length > 1_024 || hasUnsafeControl(cursor)) {
      throw usageError('Agent event cursor is invalid');
    }
    query.cursor = cursor;
  }
  if (arguments_.limit !== undefined) {
    if (!/^[0-9]+$/.test(arguments_.limit)) {
      throw usageError('Agent event limit must be an integer');
    }
    const limit = Number(arguments_.limit);
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 200) {
      throw usageError('Agent event limit must be between 1 and 200');
    }
    query.limit = limit;
  }
  return query;
}
