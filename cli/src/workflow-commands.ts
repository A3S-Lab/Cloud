import {
  type CloudApi,
  MAX_WORKFLOW_GOAL_ACL_BYTES,
  type PublishWorkflowDefinitionInput,
} from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand } from './acl-file';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  rejectLogOptions,
  requireArity,
  requireIdempotencyKey,
  requireListCommand,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import type { CommandResult } from './results';
import {
  workflowDefinitionMutationResult,
  workflowDefinitionResult,
  workflowDefinitionsResult,
  workflowGoalMutationResult,
  workflowGoalResult,
  workflowGoalsResult,
  workflowPlanRevisionResult,
  workflowRevisionResult,
  workflowRevisionsResult,
} from './workflow-results';

const MAX_WORKFLOW_PUBLICATION_FILE_BYTES = 12 * 1024 * 1024;

interface WorkflowCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executeWorkflowCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: WorkflowCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'workflow-definitions list':
      requireListCommand(arguments_);
      return workflowDefinitionsResult(
        await cloudApi().listWorkflowDefinitions(requireOrganization(context), requireProject(context))
      );
    case 'workflow-definitions get':
      requireReadCommand(arguments_, 'workflow-definitions get <workflow-definition-id>');
      return workflowDefinitionResult(
        await cloudApi().getWorkflowDefinition(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'WorkflowDefinition ID')
        )
      );
    case 'workflow-definitions create': {
      const mutation = requireAclMutationCommand(arguments_, 2, 'workflow-definitions create');
      const publication = await readWorkflowPublication(mutation.file, dependencies.readFile);
      return workflowDefinitionMutationResult(
        await cloudApi().createWorkflowDefinitionFromAcl(
          requireOrganization(context),
          requireProject(context),
          publication,
          mutation.idempotencyKey
        )
      );
    }
    case 'workflow-definitions revisions':
      requireReadCommand(arguments_, 'workflow-definitions revisions <workflow-definition-id>');
      return workflowRevisionsResult(
        await cloudApi().listWorkflowRevisions(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'WorkflowDefinition ID')
        )
      );
    case 'workflow-definitions revision':
      requireArity(
        positionals,
        4,
        'workflow-definitions revision <workflow-definition-id> <workflow-revision-id>'
      );
      rejectReadMutationOptions(arguments_);
      return workflowRevisionResult(
        await cloudApi().getWorkflowRevision(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'WorkflowDefinition ID'),
          positionalUuid(positionals, 3, 'Workflow revision ID')
        )
      );
    case 'workflow-definitions revise': {
      const mutation = requireRevisionMutation(arguments_);
      const publication = await readWorkflowPublication(mutation.file, dependencies.readFile);
      return workflowDefinitionMutationResult(
        await cloudApi().reviseWorkflowDefinitionFromAcl(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'WorkflowDefinition ID'),
          publication,
          { expectedVersion: mutation.expectedVersion },
          mutation.idempotencyKey
        )
      );
    }
    case 'workflow-goals list':
      requireListCommand(arguments_);
      return workflowGoalsResult(
        await cloudApi().listWorkflowGoals(requireOrganization(context), requireProject(context))
      );
    case 'workflow-goals get':
      requireReadCommand(arguments_, 'workflow-goals get <workflow-goal-id>');
      return workflowGoalResult(
        await cloudApi().getWorkflowGoal(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'WorkflowGoal ID')
        )
      );
    case 'workflow-goals create': {
      const mutation = requireAclMutationCommand(arguments_, 2, 'workflow-goals create');
      const acl = await readAclDocument(
        mutation.file,
        { label: 'Workflow goal ACL', maximumBytes: MAX_WORKFLOW_GOAL_ACL_BYTES },
        dependencies.readFile
      );
      return workflowGoalMutationResult(
        await cloudApi().createWorkflowGoalFromAcl(
          requireOrganization(context),
          requireProject(context),
          acl,
          mutation.idempotencyKey
        )
      );
    }
    case 'workflow-goals plan':
      requireArity(positionals, 4, 'workflow-goals plan <workflow-goal-id> <plan-revision-id>');
      rejectReadMutationOptions(arguments_);
      return workflowPlanRevisionResult(
        await cloudApi().getWorkflowPlanRevision(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'WorkflowGoal ID'),
          positionalUuid(positionals, 3, 'Plan revision ID')
        )
      );
    default:
      return undefined;
  }
}

function requireRevisionMutation(arguments_: ParsedArguments): {
  expectedVersion: number;
  idempotencyKey: string;
  file: string;
} {
  requireArity(arguments_.positionals, 3, 'workflow-definitions revise <workflow-definition-id>');
  rejectLogOptions(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  const idempotencyKey = requireIdempotencyKey(arguments_);
  const file = arguments_.file;
  if (file === undefined || file.length > 4_096 || /[\0\r\n]/.test(file)) {
    throw usageError('--file with a valid Workflow publication path is required');
  }
  const rawVersion = arguments_.expectedVersion;
  if (rawVersion === undefined || !/^[0-9]+$/.test(rawVersion)) {
    throw usageError('--expected-version must be a positive safe integer for Workflow revision');
  }
  const expectedVersion = Number(rawVersion);
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw usageError('--expected-version must be a positive safe integer for Workflow revision');
  }
  return { expectedVersion, idempotencyKey, file };
}

function rejectReadMutationOptions(arguments_: ParsedArguments): void {
  rejectLogOptions(arguments_);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  if (arguments_.expectedVersion !== undefined) {
    throw usageError('--expected-version is valid only for workflow-definitions revise');
  }
  rejectGatewayRolloutOptions(arguments_);
}

async function readWorkflowPublication(
  path: string,
  readFile: (path: string) => Promise<Uint8Array> = (value) => Bun.file(value).bytes()
): Promise<PublishWorkflowDefinitionInput> {
  let bytes: Uint8Array;
  try {
    bytes = await readFile(path);
  } catch {
    throw usageError('unable to read the Workflow publication file');
  }
  if (bytes.byteLength < 1 || bytes.byteLength > MAX_WORKFLOW_PUBLICATION_FILE_BYTES) {
    throw usageError(
      `Workflow publication must contain between 1 and ${MAX_WORKFLOW_PUBLICATION_FILE_BYTES} UTF-8 bytes`
    );
  }
  let decoded: string;
  try {
    decoded = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    throw usageError('Workflow publication must be valid UTF-8');
  }
  let value: unknown;
  try {
    value = JSON.parse(decoded);
  } catch {
    throw usageError('Workflow publication must be valid JSON transport');
  }
  if (!isWorkflowPublication(value)) {
    throw usageError('Workflow publication must contain only definitionAcl and typed ACL payloads');
  }
  return value;
}

function isWorkflowPublication(value: unknown): value is PublishWorkflowDefinitionInput {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return false;
  }
  const record = value as Record<string, unknown>;
  if (
    Object.keys(record).some((key) => !['definitionAcl', 'payloads'].includes(key)) ||
    typeof record.definitionAcl !== 'string' ||
    !Array.isArray(record.payloads)
  ) {
    return false;
  }
  return record.payloads.every((payload) => {
    if (typeof payload !== 'object' || payload === null || Array.isArray(payload)) {
      return false;
    }
    const entry = payload as Record<string, unknown>;
    return (
      !Object.keys(entry).some((key) => !['kind', 'acl'].includes(key)) &&
      typeof entry.kind === 'string' &&
      ['configuration', 'data_schema', 'policy'].includes(entry.kind) &&
      typeof entry.acl === 'string'
    );
  });
}
