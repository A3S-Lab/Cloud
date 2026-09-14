import {
  type CloudApi,
  DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT,
  encodeAutomationDefinitionListOptions,
  MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT,
  type AutomationDefinitionListOptions,
} from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import {
  positionalResourceName,
  positionalUuid,
  rejectAgentProviderKindOption,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  requireArity,
  requireBoundedListReadCommand,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireEnvironment, requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import {
  automationDefinitionResult,
  automationDefinitionsResult,
  automationRevisionResult,
  automationWebhookEndpointResult,
} from './automation-results';
import type { CommandResult } from './results';

export async function executeAutomationCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  const organizationId = () => requireOrganization(context);
  const projectId = () => requireProject(context);
  const environmentId = () => requireEnvironment(context);
  switch (command) {
    case 'automation-webhook-endpoints create': {
      requireArity(
        positionals,
        9,
        'automation-webhook-endpoints create <endpoint-id> <endpoint-key> <secret-id> <secret-version> <max-body-bytes> <automation-id> <revision-id>'
      );
      rejectLogAndFenceOptions(arguments_);
      rejectIdempotencyOption(arguments_);
      rejectFileOption(arguments_);
      const secretVersion = parsePositiveInteger(positionals[5]!, 'Secret version');
      const maxBodyBytes = parsePositiveInteger(positionals[6]!, 'Max body bytes');
      return automationWebhookEndpointResult(
        await cloudApi().createAutomationWebhookEndpoint(
          organizationId(),
          projectId(),
          environmentId(),
          {
            endpointId: positionalUuid(positionals, 2, 'Automation webhook endpoint ID'),
            endpointKey: positionalResourceName(positionals, 3),
            signingSecret: {
              secretId: positionalUuid(positionals, 4, 'Secret ID'),
              version: secretVersion,
            },
            maxBodyBytes,
            automationId: positionalUuid(positionals, 7, 'Automation ID'),
            revisionId: positionalUuid(positionals, 8, 'Automation revision ID'),
          }
        )
      );
    }
    case 'automation-webhook-endpoints get':
      requireReadCommand(arguments_, 'automation-webhook-endpoints get <endpoint-id>');
      return automationWebhookEndpointResult(
        await cloudApi().getAutomationWebhookEndpoint(
          organizationId(),
          projectId(),
          environmentId(),
          positionalUuid(positionals, 2, 'Automation webhook endpoint ID')
        )
      );
    case 'automation-webhook-endpoints disable':
    case 'automation-webhook-endpoints enable':
    case 'automation-webhook-endpoints revoke': {
      const action = command.split(' ')[1]!;
      requireArity(positionals, 3, `automation-webhook-endpoints ${action} <endpoint-id>`);
      rejectLogAndFenceOptions(arguments_, { allowExpectedVersion: true });
      rejectIdempotencyOption(arguments_);
      rejectFileOption(arguments_);
      const expectedGeneration = requireExpectedGeneration(arguments_);
      const endpointId = positionalUuid(positionals, 2, 'Automation webhook endpoint ID');
      const input = { expectedGeneration };
      const api = cloudApi();
      const result =
        action === 'disable'
          ? await api.disableAutomationWebhookEndpoint(
              organizationId(),
              projectId(),
              environmentId(),
              endpointId,
              input
            )
          : action === 'enable'
            ? await api.enableAutomationWebhookEndpoint(
                organizationId(),
                projectId(),
                environmentId(),
                endpointId,
                input
              )
            : await api.revokeAutomationWebhookEndpoint(
                organizationId(),
                projectId(),
                environmentId(),
                endpointId,
                input
              );
      return automationWebhookEndpointResult(result);
    }
    case 'automation-definitions list': {
      const limit = requireBoundedListReadCommand(
        arguments_,
        'automation-definitions list',
        2,
        DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT,
        MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT,
        'Automation definition'
      );
      const options: AutomationDefinitionListOptions = { limit };
      try {
        encodeAutomationDefinitionListOptions(options);
      } catch (error) {
        if (error instanceof Error) {
          throw usageError(error.message);
        }
        throw error;
      }
      return automationDefinitionsResult(
        await cloudApi().listAutomationDefinitions(organizationId(), options)
      );
    }
    case 'automation-definitions get':
      requireReadCommand(arguments_, 'automation-definitions get <automation-id>');
      return automationDefinitionResult(
        await cloudApi().getAutomationDefinition(
          organizationId(),
          positionalUuid(positionals, 2, 'Automation ID')
        )
      );
    case 'automation-revisions get':
      requireReadCommand(
        arguments_,
        'automation-revisions get <automation-id> <revision-id>',
        4
      );
      return automationRevisionResult(
        await cloudApi().getAutomationRevision(
          organizationId(),
          positionalUuid(positionals, 2, 'Automation ID'),
          positionalUuid(positionals, 3, 'Automation revision ID')
        )
      );
    default:
      return undefined;
  }
}

function rejectLogAndFenceOptions(
  arguments_: ParsedArguments,
  options: { allowExpectedVersion?: boolean } = {}
): void {
  rejectAgentProviderKindOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
    throw usageError('cursor and stream options are valid only for log commands');
  }
  if (!options.allowExpectedVersion) {
    rejectExpectedVersionOption(arguments_);
  }
}

function requireExpectedGeneration(arguments_: ParsedArguments): number {
  const raw = arguments_.expectedVersion;
  if (raw === undefined || !/^[0-9]+$/u.test(raw)) {
    throw usageError(
      '--expected-version must be a non-negative safe integer for Automation webhook lifecycle mutation'
    );
  }
  const expectedGeneration = Number(raw);
  if (!Number.isSafeInteger(expectedGeneration) || expectedGeneration < 0) {
    throw usageError(
      '--expected-version must be a non-negative safe integer for Automation webhook lifecycle mutation'
    );
  }
  return expectedGeneration;
}

function parsePositiveInteger(value: string, label: string): number {
  if (!/^[0-9]+$/u.test(value)) {
    throw usageError(`${label} must be a positive safe integer`);
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 1) {
    throw usageError(`${label} must be a positive safe integer`);
  }
  return parsed;
}
