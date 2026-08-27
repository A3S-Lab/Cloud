import {
  type CloudApi,
  DEFAULT_BUILD_PLAN_LIST_LIMIT,
  MAX_BUILD_PLAN_LIST_LIMIT,
  MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES,
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
  buildPlanDetectionResult,
  buildPlanMutationResult,
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
    default:
      return undefined;
  }
}

function requireBuildPlanListCommand(arguments_: ParsedArguments): void {
  requireArity(arguments_.positionals, 3, 'build-plans list <source-revision-id>');
  if (arguments_.cursor !== undefined || arguments_.stream !== undefined) {
    throw usageError('cursor and stream options are not valid for BuildPlan reads');
  }
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
  rejectExpectedVersionOption(arguments_);
  rejectGatewayRolloutOptions(arguments_);
  rejectAgentProviderKindOption(arguments_);
}

function buildPlanListLimit(raw: string | undefined): number {
  if (raw === undefined) {
    return DEFAULT_BUILD_PLAN_LIST_LIMIT;
  }
  if (!/^[0-9]+$/u.test(raw)) {
    throw usageError(`BuildPlan list limit must be between 1 and ${MAX_BUILD_PLAN_LIST_LIMIT}`);
  }
  const limit = Number(raw);
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_BUILD_PLAN_LIST_LIMIT) {
    throw usageError(`BuildPlan list limit must be between 1 and ${MAX_BUILD_PLAN_LIST_LIMIT}`);
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
