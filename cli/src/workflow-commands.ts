import {
  type CloudApi,
  type HumanTaskInteractionSubmission,
  type HumanTaskStatus,
  MAX_HUMAN_TASK_LIST_LIMIT,
  MAX_WORKFLOW_GOAL_ACL_BYTES,
  MAX_WORKFLOW_RUN_HISTORY_LIMIT,
  MAX_WORKFLOW_RUN_LIST_LIMIT,
  MAX_WORKFLOW_RUN_TIMEOUT_SECONDS,
  MAX_WORKFLOW_RUN_WAIT_SECONDS,
  type PublishWorkflowDefinitionInput,
} from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand } from './acl-file';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  rejectLogOptions,
  requireArity,
  requireIdempotencyKey,
  requireListCommand,
  requireMutationCommand,
  requireReadCommand,
  requireVersionedMutationCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireOrganization, requireProject } from './context';
import { usageError } from './errors';
import { isJsonObject, readBoundedJsonFile } from './json-file';
import type { CommandResult } from './results';
import {
  humanTaskMutationResult,
  humanTaskResult,
  humanTasksResult,
  workflowDefinitionMutationResult,
  workflowDefinitionResult,
  workflowDefinitionsResult,
  workflowGoalMutationResult,
  workflowGoalResult,
  workflowGoalsResult,
  workflowPlanRevisionResult,
  workflowRevisionResult,
  workflowRevisionsResult,
  workflowRunHistoryResult,
  workflowRunMutationResult,
  workflowRunOutputResult,
  workflowRunResult,
  workflowRunsResult,
} from './workflow-results';

const MAX_WORKFLOW_PUBLICATION_FILE_BYTES = 12 * 1024 * 1024;
const MAX_HUMAN_TASK_SUBMISSION_FILE_BYTES = 5 * 1024 * 1024;

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
  if (!command.startsWith('workflow-runs ')) {
    rejectMisplacedWorkflowRunOptions(arguments_);
  }
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
    case 'human-tasks list': {
      if (positionals.length < 2 || positionals.length > 3) {
        throw usageError('usage: a3s-cloud human-tasks list [status]');
      }
      rejectWorkflowRunReadMutationOptions(arguments_);
      if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
        throw usageError('--cursor and --stream are not valid for HumanTask list');
      }
      const status = parseHumanTaskStatus(positionals[2]);
      const limit = parseBoundedIntegerOption(
        arguments_.limit,
        'HumanTask list limit',
        1,
        MAX_HUMAN_TASK_LIST_LIMIT
      );
      return humanTasksResult(
        await cloudApi().listHumanTasks(requireOrganization(context), requireProject(context), {
          status,
          limit,
        })
      );
    }
    case 'human-tasks get':
      requireReadCommand(arguments_, 'human-tasks get <human-task-id>');
      return humanTaskResult(
        await cloudApi().getHumanTask(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'HumanTask ID')
        )
      );
    case 'human-tasks claim':
    case 'human-tasks release': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        3,
        `${command} <human-task-id>`,
        'HumanTask'
      );
      const organizationId = requireOrganization(context);
      const humanTaskId = positionalUuid(positionals, 2, 'HumanTask ID');
      const api = cloudApi();
      return humanTaskMutationResult(
        await (command === 'human-tasks claim'
          ? api.claimHumanTask(organizationId, humanTaskId, mutation.expectedVersion, mutation.idempotencyKey)
          : api.releaseHumanTask(
              organizationId,
              humanTaskId,
              mutation.expectedVersion,
              mutation.idempotencyKey
            ))
      );
    }
    case 'human-tasks submit': {
      requireArity(positionals, 3, 'human-tasks submit <human-task-id>');
      rejectLogOptions(arguments_);
      rejectIdempotencyOption(arguments_);
      rejectExpectedVersionOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      const file = arguments_.file;
      if (file === undefined || file.length > 4_096 || /[\0\r\n]/.test(file)) {
        throw usageError('--file with a native A3S Form interaction submission is required');
      }
      const submission = await readHumanTaskSubmission(file, dependencies.readFile);
      return humanTaskMutationResult(
        await cloudApi().submitHumanTask(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'HumanTask ID'),
          submission
        )
      );
    }
    case 'workflow-runs list': {
      requireArity(positionals, 2, 'workflow-runs list');
      rejectWorkflowRunReadMutationOptions(arguments_);
      if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
        throw usageError('--cursor and --stream are not valid for WorkflowRun list');
      }
      rejectWorkflowRunSpecificOptions(arguments_);
      const limit = parseBoundedIntegerOption(
        arguments_.limit,
        'WorkflowRun list limit',
        1,
        MAX_WORKFLOW_RUN_LIST_LIMIT
      );
      return workflowRunsResult(
        await cloudApi().listWorkflowRuns(requireOrganization(context), requireProject(context), { limit })
      );
    }
    case 'workflow-runs get':
      requireReadCommand(arguments_, 'workflow-runs get <workflow-run-id>');
      rejectWorkflowRunSpecificOptions(arguments_);
      return workflowRunResult(
        await cloudApi().getWorkflowRun(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'WorkflowRun ID')
        )
      );
    case 'workflow-runs start': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        4,
        'workflow-runs start <workflow-goal-id> <plan-revision-id>'
      );
      if (arguments_.reason !== undefined || arguments_.workflowRunWaitSeconds !== undefined) {
        throw usageError('--reason and --wait-seconds are not valid for WorkflowRun start');
      }
      const timeoutSeconds = parseBoundedIntegerOption(
        arguments_.workflowRunTimeoutSeconds,
        'WorkflowRun timeout',
        1,
        MAX_WORKFLOW_RUN_TIMEOUT_SECONDS
      );
      return workflowRunMutationResult(
        await cloudApi().startWorkflowRun(
          requireOrganization(context),
          requireProject(context),
          {
            workflowGoalId: positionalUuid(positionals, 2, 'WorkflowGoal ID'),
            planRevisionId: positionalUuid(positionals, 3, 'Plan revision ID'),
            timeoutSeconds,
          },
          idempotencyKey
        )
      );
    }
    case 'workflow-runs cancel': {
      const idempotencyKey = requireMutationCommand(arguments_, 3, 'workflow-runs cancel <workflow-run-id>');
      if (
        arguments_.workflowRunTimeoutSeconds !== undefined ||
        arguments_.workflowRunWaitSeconds !== undefined
      ) {
        throw usageError('--run-timeout-seconds and --wait-seconds are not valid for WorkflowRun cancel');
      }
      return workflowRunMutationResult(
        await cloudApi().cancelWorkflowRun(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'WorkflowRun ID'),
          { reason: arguments_.reason },
          idempotencyKey
        )
      );
    }
    case 'workflow-runs wait': {
      requireReadCommand(arguments_, 'workflow-runs wait <workflow-run-id>');
      if (arguments_.reason !== undefined || arguments_.workflowRunTimeoutSeconds !== undefined) {
        throw usageError('--reason and --run-timeout-seconds are not valid for WorkflowRun wait');
      }
      const timeoutSeconds = parseBoundedIntegerOption(
        arguments_.workflowRunWaitSeconds,
        'WorkflowRun wait timeout',
        0,
        MAX_WORKFLOW_RUN_WAIT_SECONDS
      );
      return workflowRunResult(
        await cloudApi().waitWorkflowRun(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'WorkflowRun ID'),
          { timeoutSeconds }
        )
      );
    }
    case 'workflow-runs output':
      requireReadCommand(arguments_, 'workflow-runs output <workflow-run-id>');
      rejectWorkflowRunSpecificOptions(arguments_);
      return workflowRunOutputResult(
        await cloudApi().getWorkflowRunOutput(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'WorkflowRun ID')
        )
      );
    case 'workflow-runs history': {
      requireArity(positionals, 3, 'workflow-runs history <workflow-run-id>');
      rejectWorkflowRunReadMutationOptions(arguments_);
      if (arguments_.stream !== undefined) {
        throw usageError('--stream is not valid for WorkflowRun history');
      }
      rejectWorkflowRunSpecificOptions(arguments_);
      const afterSequence = parseBoundedIntegerOption(
        arguments_.cursor,
        'WorkflowRun history cursor',
        0,
        Number.MAX_SAFE_INTEGER
      );
      const limit = parseBoundedIntegerOption(
        arguments_.limit,
        'WorkflowRun history limit',
        1,
        MAX_WORKFLOW_RUN_HISTORY_LIMIT
      );
      return workflowRunHistoryResult(
        await cloudApi().getWorkflowRunHistory(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'WorkflowRun ID'),
          { afterSequence, limit }
        )
      );
    }
    default:
      return undefined;
  }
}

function parseHumanTaskStatus(raw: string | undefined): HumanTaskStatus | undefined {
  if (raw === undefined) {
    return undefined;
  }
  if (!['pending_activation', 'ready', 'claimed', 'completed', 'expired', 'cancelled'].includes(raw)) {
    throw usageError('HumanTask status is invalid');
  }
  return raw as HumanTaskStatus;
}

function rejectMisplacedWorkflowRunOptions(arguments_: ParsedArguments): void {
  if (
    arguments_.workflowRunTimeoutSeconds !== undefined ||
    arguments_.workflowRunWaitSeconds !== undefined ||
    arguments_.reason !== undefined
  ) {
    throw usageError(
      '--run-timeout-seconds, --wait-seconds, and --reason are valid only for WorkflowRun commands'
    );
  }
}

function rejectWorkflowRunSpecificOptions(arguments_: ParsedArguments): void {
  rejectMisplacedWorkflowRunOptions(arguments_);
}

function rejectWorkflowRunReadMutationOptions(arguments_: ParsedArguments): void {
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
}

function parseBoundedIntegerOption(
  raw: string | undefined,
  label: string,
  minimum: number,
  maximum: number
): number | undefined {
  if (raw === undefined) {
    return undefined;
  }
  if (!/^[0-9]+$/u.test(raw)) {
    throw usageError(`${label} must be an integer`);
  }
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value < minimum || value > maximum) {
    throw usageError(`${label} must be between ${minimum} and ${maximum}`);
  }
  return value;
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
  const value = await readBoundedJsonFile(
    path,
    {
      label: 'Workflow publication',
      maximumBytes: MAX_WORKFLOW_PUBLICATION_FILE_BYTES,
      readError: 'unable to read the Workflow publication file',
    },
    readFile
  );
  if (!isWorkflowPublication(value)) {
    throw usageError('Workflow publication must contain only definitionAcl and typed ACL payloads');
  }
  return value;
}

async function readHumanTaskSubmission(
  path: string,
  readFile?: (path: string) => Promise<Uint8Array>
): Promise<HumanTaskInteractionSubmission> {
  const value = await readBoundedJsonFile(
    path,
    {
      label: 'HumanTask Form interaction submission',
      maximumBytes: MAX_HUMAN_TASK_SUBMISSION_FILE_BYTES,
      readError: 'unable to read the HumanTask Form interaction submission file',
    },
    readFile
  );
  if (!isJsonObject(value) || value.apiVersion !== 'a3s.dev/form-interaction-submission/v1') {
    throw usageError('HumanTask submission must be a native A3S Form interaction submission');
  }
  return value as unknown as HumanTaskInteractionSubmission;
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
