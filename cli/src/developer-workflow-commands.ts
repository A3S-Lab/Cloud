import {
  type CloudApi,
  DEFAULT_BUILD_PLAN_LIST_LIMIT,
  DEFAULT_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
  MAX_BUILD_PLAN_LIST_LIMIT,
  MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES,
  MAX_WORKLOAD_PROFILE_ACL_BYTES,
  MAX_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
} from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand } from './acl-file';
import type { ParsedArguments } from './arguments';
import {
  positionalUuid,
  rejectAgentProviderKindOption,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  requireArity,
  requireReadCommand,
} from './command-options';
import type { CloudContext } from './context';
import { requireEnvironment, requireOrganization, requireProject } from './context';
import {
  acceptedBuildPlanResult,
  acceptedBuildPlansResult,
  acceptedWorkloadProfileRevisionResult,
  acceptedWorkloadProfileRevisionsResult,
  buildPlanDetectionResult,
  buildPlanMutationResult,
  workloadProfileMutationResult,
} from './developer-workflow-results';
import { usageError } from './errors';
import type { CommandResult } from './results';

interface DeveloperWorkflowCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executeDeveloperWorkflowCommand(
  command: string,
  arguments_: ParsedArguments,
  context: CloudContext,
  cloudApi: () => CloudApi,
  dependencies: DeveloperWorkflowCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'build-plan-detections create': {
      requireReadCommand(arguments_, 'build-plan-detections create <source-revision-id>', 3);
      const scope = requireEnvironmentScope(context);
      return buildPlanDetectionResult(
        await cloudApi().detectBuildPlans(scope.organizationId, scope.projectId, scope.environmentId, {
          sourceRevisionId: positionalUuid(positionals, 2, 'Source revision ID'),
        })
      );
    }
    case 'build-plans accept': {
      const mutation = requireAclMutationCommand(arguments_, 3, 'build-plans accept <source-revision-id>');
      rejectAgentProviderKindOption(arguments_);
      const proposalAcl = await readAclDocument(
        mutation.file,
        {
          label: 'BuildPlan proposal ACL',
          maximumBytes: MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES,
        },
        dependencies.readFile
      );
      const scope = requireEnvironmentScope(context);
      return buildPlanMutationResult(
        await cloudApi().acceptBuildPlan(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          {
            sourceRevisionId: positionalUuid(positionals, 2, 'Source revision ID'),
            proposalAcl,
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'build-plans list': {
      requireBuildPlanListCommand(arguments_);
      const scope = requireEnvironmentScope(context);
      return acceptedBuildPlansResult(
        await cloudApi().listAcceptedBuildPlans(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          positionalUuid(positionals, 2, 'Source revision ID'),
          buildPlanListLimit(arguments_.limit)
        )
      );
    }
    case 'build-plans get': {
      requireReadCommand(arguments_, 'build-plans get <build-plan-id>', 3);
      const scope = requireEnvironmentScope(context);
      return acceptedBuildPlanResult(
        await cloudApi().getAcceptedBuildPlan(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          positionalUuid(positionals, 2, 'BuildPlan ID')
        )
      );
    }
    case 'workload-profiles accept': {
      const mutation = requireAclMutationCommand(arguments_, 3, 'workload-profiles accept <build-plan-id>');
      rejectAgentProviderKindOption(arguments_);
      const profileAcl = await readAclDocument(
        mutation.file,
        {
          label: 'WorkloadProfile ACL',
          maximumBytes: MAX_WORKLOAD_PROFILE_ACL_BYTES,
        },
        dependencies.readFile
      );
      const scope = requireEnvironmentScope(context);
      return workloadProfileMutationResult(
        await cloudApi().acceptWorkloadProfile(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          {
            buildPlanId: positionalUuid(positionals, 2, 'BuildPlan ID'),
            profileAcl,
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'workload-profiles get': {
      requireReadCommand(arguments_, 'workload-profiles get <workload-profile-id>', 3);
      const scope = requireEnvironmentScope(context);
      return acceptedWorkloadProfileRevisionResult(
        await cloudApi().getCurrentAcceptedWorkloadProfileRevision(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          positionalUuid(positionals, 2, 'WorkloadProfile ID')
        )
      );
    }
    case 'workload-profile-revisions list': {
      requireDeveloperWorkflowListCommand(
        arguments_,
        'workload-profile-revisions list <workload-profile-id>',
        3,
        'WorkloadProfile revision reads'
      );
      const scope = requireEnvironmentScope(context);
      return acceptedWorkloadProfileRevisionsResult(
        await cloudApi().listAcceptedWorkloadProfileRevisions(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          positionalUuid(positionals, 2, 'WorkloadProfile ID'),
          boundedListLimit(
            arguments_.limit,
            DEFAULT_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
            MAX_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
            'WorkloadProfile revision list limit'
          )
        )
      );
    }
    case 'workload-profile-revisions get': {
      requireReadCommand(arguments_, 'workload-profile-revisions get <workload-profile-id> <revision-id>', 4);
      const scope = requireEnvironmentScope(context);
      return acceptedWorkloadProfileRevisionResult(
        await cloudApi().getAcceptedWorkloadProfileRevision(
          scope.organizationId,
          scope.projectId,
          scope.environmentId,
          positionalUuid(positionals, 2, 'WorkloadProfile ID'),
          positionalUuid(positionals, 3, 'WorkloadProfile revision ID')
        )
      );
    }
    default:
      return undefined;
  }
}

function requireBuildPlanListCommand(arguments_: ParsedArguments): void {
  requireDeveloperWorkflowListCommand(
    arguments_,
    'build-plans list <source-revision-id>',
    3,
    'BuildPlan reads'
  );
}

function requireDeveloperWorkflowListCommand(
  arguments_: ParsedArguments,
  usage: string,
  arity: number,
  label: string
): void {
  requireArity(arguments_.positionals, arity, usage);
  if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
    throw usageError(`cursor and stream options are not valid for ${label}`);
  }
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  rejectAgentProviderKindOption(arguments_);
}

function buildPlanListLimit(raw: string | undefined): number {
  return boundedListLimit(
    raw,
    DEFAULT_BUILD_PLAN_LIST_LIMIT,
    MAX_BUILD_PLAN_LIST_LIMIT,
    'BuildPlan list limit'
  );
}

function boundedListLimit(
  raw: string | undefined,
  defaultValue: number,
  maximum: number,
  label: string
): number {
  if (raw === undefined) {
    return defaultValue;
  }
  if (!/^[0-9]+$/u.test(raw)) {
    throw usageError(`${label} must be between 1 and ${maximum}`);
  }
  const limit = Number(raw);
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > maximum) {
    throw usageError(`${label} must be between 1 and ${maximum}`);
  }
  return limit;
}

function requireEnvironmentScope(context: CloudContext): {
  organizationId: string;
  projectId: string;
  environmentId: string;
} {
  return {
    organizationId: requireOrganization(context),
    projectId: requireProject(context),
    environmentId: requireEnvironment(context),
  };
}
