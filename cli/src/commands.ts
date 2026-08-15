import { CloudApi, type CloudFetch, type CloudLogQuery, MAX_WORKLOAD_ACL_BYTES } from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand } from './acl-file';
import { executeAgentCommand } from './agent-commands';
import type { ParsedArguments } from './arguments';
import { executeAssetCommand } from './asset-commands';
import { executeAuditCommand, rejectMisplacedAuditOptions } from './audit-commands';
import {
  positionalResourceName,
  positionalUuid,
  rejectExpectedVersionOption,
  rejectFileOption,
  rejectGatewayRolloutOptions,
  rejectIdempotencyOption,
  rejectLogOptions,
  requireArity,
  requireListCommand,
  requireMutationCommand,
  requireReadCommand,
  requireVersionedMutationCommand,
} from './command-options';
import { executeConnectorCommand } from './connector-commands';
import type { CloudContext } from './context';
import {
  hasUnsafeControl,
  publicContext,
  requireEnvironment,
  requireOrganization,
  requireProject,
  requireToken,
} from './context';
import { executeEdgeCommand } from './edge-commands';
import { usageError } from './errors';
import { executeExecutionTemplateCommand } from './execution-template-commands';
import { executeFormCommand } from './form-commands';
import { executeIdentityCommand, rejectMisplacedIdentityOptions } from './identity-commands';
import { executeNodeCommand, rejectMisplacedNodeOptions } from './node-commands';
import { executeNotificationCommand, rejectMisplacedNotificationOptions } from './notification-commands';
import { executeOntologyCommand } from './ontology-commands';
import { executePluginCommand } from './plugin-commands';
import {
  buildEvidenceResult,
  buildRunLogsResult,
  buildRunResult,
  buildRunsResult,
  type CommandResult,
  cancelBuildRunResult,
  cancelDeploymentResult,
  contextResult,
  deploymentResult,
  diagnosticsResult,
  environmentMutationResult,
  environmentsResult,
  operationsResult,
  organizationMutationResult,
  organizationsResult,
  projectAttributionMutationResult,
  projectAttributionResult,
  projectMutationResult,
  projectsResult,
  retryBuildRunResult,
  routeResult,
  routesResult,
  stopWorkloadResult,
  workloadDeploymentResult,
  workloadLogsResult,
  workloadResult,
  workloadsResult,
} from './results';
import { executeSearchCommand } from './search-commands';
import { executeSecretCommand, rejectMisplacedSecretValueOption } from './secret-commands';
import { executeSourceCommand, rejectMisplacedSourceRecipeOptions } from './source-commands';
import type { ReadStdin } from './standard-input';
import { executeWorkflowCommand } from './workflow-commands';

export interface CommandDependencies {
  fetch?: CloudFetch;
  readFile?: (path: string) => Promise<Uint8Array>;
  readStdin?: ReadStdin;
}

export async function executeCommand(
  arguments_: ParsedArguments,
  context: CloudContext,
  dependencies: CommandDependencies = {}
): Promise<CommandResult> {
  const { positionals } = arguments_;
  if (positionals.length < 2) {
    throw usageError('a command and action are required; run a3s-cloud --help');
  }
  const command = `${positionals[0]} ${positionals[1]}`;
  if (arguments_.migrationRuleId !== undefined && command !== 'ontologies revise') {
    throw usageError('--migration-rule is valid only for ontologies revise');
  }
  rejectMisplacedSourceRecipeOptions(command, arguments_);
  rejectMisplacedSecretValueOption(command, arguments_);
  rejectMisplacedIdentityOptions(command, arguments_);
  rejectMisplacedNodeOptions(command, arguments_);
  rejectMisplacedAuditOptions(command, arguments_);
  rejectMisplacedNotificationOptions(command, arguments_);
  rejectMisplacedProjectAttributionOptions(command, arguments_);
  if (command === 'context show') {
    requireArity(positionals, 2, 'context show');
    rejectLogOptions(arguments_);
    rejectIdempotencyOption(arguments_);
    rejectFileOption(arguments_);
    rejectExpectedVersionOption(arguments_);
    rejectGatewayRolloutOptions(arguments_);
    return contextResult(publicContext(context));
  }
  if (command === 'diagnostics status') {
    requireArity(positionals, 2, 'diagnostics status');
    rejectLogOptions(arguments_);
    rejectIdempotencyOption(arguments_);
    rejectFileOption(arguments_);
    rejectExpectedVersionOption(arguments_);
    rejectGatewayRolloutOptions(arguments_);
    const api = new CloudApi(undefined, context.baseUrl, {
      fetch: dependencies.fetch,
      requestTimeoutMs: context.timeoutMs,
    });
    return diagnosticsResult(await api.getDiagnostics());
  }

  let api: CloudApi | undefined;
  const cloudApi = (): CloudApi => {
    api ??= new CloudApi(requireToken(context), context.baseUrl, {
      fetch: dependencies.fetch,
      requestTimeoutMs: context.timeoutMs,
    });
    return api;
  };
  const searchResult = await executeSearchCommand(command, arguments_, context, cloudApi);
  if (searchResult !== undefined) {
    return searchResult;
  }
  const auditResult = await executeAuditCommand(command, arguments_, context, cloudApi);
  if (auditResult !== undefined) {
    return auditResult;
  }
  const notificationResult = await executeNotificationCommand(command, arguments_, context, cloudApi);
  if (notificationResult !== undefined) {
    return notificationResult;
  }
  const ontologyResult = await executeOntologyCommand(command, arguments_, context, cloudApi, {
    readFile: dependencies.readFile,
  });
  if (ontologyResult !== undefined) {
    return ontologyResult;
  }
  const workflowResult = await executeWorkflowCommand(command, arguments_, context, cloudApi, {
    readFile: dependencies.readFile,
  });
  if (workflowResult !== undefined) {
    return workflowResult;
  }
  const formResult = await executeFormCommand(command, arguments_, context, cloudApi, {
    readFile: dependencies.readFile,
  });
  if (formResult !== undefined) {
    return formResult;
  }
  const executionTemplateResult = await executeExecutionTemplateCommand(
    command,
    arguments_,
    context,
    cloudApi,
    { readFile: dependencies.readFile }
  );
  if (executionTemplateResult !== undefined) {
    return executionTemplateResult;
  }
  const connectorResult = await executeConnectorCommand(command, arguments_, context, cloudApi, {
    readFile: dependencies.readFile,
  });
  if (connectorResult !== undefined) {
    return connectorResult;
  }
  const edgeResult = await executeEdgeCommand(command, arguments_, context, cloudApi, {
    readFile: dependencies.readFile,
  });
  if (edgeResult !== undefined) {
    return edgeResult;
  }
  const sourceResult = await executeSourceCommand(command, arguments_, context, cloudApi);
  if (sourceResult !== undefined) {
    return sourceResult;
  }
  const identityResult = await executeIdentityCommand(command, arguments_, context, cloudApi, {
    readStdin: dependencies.readStdin,
  });
  if (identityResult !== undefined) {
    return identityResult;
  }
  const secretResult = await executeSecretCommand(command, arguments_, context, cloudApi, {
    readStdin: dependencies.readStdin,
  });
  if (secretResult !== undefined) {
    return secretResult;
  }
  const nodeResult = await executeNodeCommand(command, arguments_, context, cloudApi, {
    readStdin: dependencies.readStdin,
  });
  if (nodeResult !== undefined) {
    return nodeResult;
  }
  const agentResult = await executeAgentCommand(command, arguments_, context, cloudApi);
  if (agentResult !== undefined) {
    return agentResult;
  }
  const assetResult = await executeAssetCommand(command, arguments_, context, cloudApi, {
    readFile: dependencies.readFile,
  });
  if (assetResult !== undefined) {
    return assetResult;
  }
  const pluginResult = await executePluginCommand(command, arguments_, context, cloudApi, {
    readFile: dependencies.readFile,
  });
  if (pluginResult !== undefined) {
    return pluginResult;
  }
  switch (command) {
    case 'organizations list':
      requireListCommand(arguments_);
      return organizationsResult(await cloudApi().listOrganizations());
    case 'organizations create': {
      const mutation = requireNamedMutationCommand(arguments_, 'organizations create <name>');
      return organizationMutationResult(
        await cloudApi().createOrganization(mutation.name, mutation.idempotencyKey)
      );
    }
    case 'projects list':
      requireListCommand(arguments_);
      return projectsResult(await cloudApi().listProjects(requireOrganization(context)));
    case 'projects create': {
      const mutation = requireNamedMutationCommand(arguments_, 'projects create <name>');
      return projectMutationResult(
        await cloudApi().createProject(requireOrganization(context), mutation.name, mutation.idempotencyKey)
      );
    }
    case 'project-attribution get': {
      if (positionals.length !== 2 && positionals.length !== 3) {
        throw usageError('usage: a3s-cloud project-attribution get [profile-id]');
      }
      requireReadCommand(arguments_, 'project-attribution get [profile-id]', positionals.length);
      const organizationId = requireOrganization(context);
      const projectId = requireProject(context);
      const profileId =
        positionals.length === 3
          ? positionalUuid(positionals, 2, 'project attribution profile ID')
          : undefined;
      return projectAttributionResult(
        profileId
          ? await cloudApi().getProjectAttributionRevision(organizationId, projectId, profileId)
          : await cloudApi().getProjectAttribution(organizationId, projectId)
      );
    }
    case 'project-attribution update': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        3,
        'project-attribution update <business-owner-reference>',
        'project attribution'
      );
      return projectAttributionMutationResult(
        await cloudApi().updateProjectAttribution(
          requireOrganization(context),
          requireProject(context),
          {
            businessOwnerReference: positionals[2] as string,
            ...(arguments_.costAttributionCode === undefined
              ? {}
              : { costAttributionCode: arguments_.costAttributionCode }),
            labels: parseProjectAttributionLabels(arguments_.projectAttributionLabels),
          },
          mutation.expectedVersion,
          mutation.idempotencyKey
        )
      );
    }
    case 'environments list':
      requireListCommand(arguments_);
      return environmentsResult(
        await cloudApi().listEnvironments(requireOrganization(context), requireProject(context))
      );
    case 'environments create': {
      const mutation = requireNamedMutationCommand(arguments_, 'environments create <name>');
      return environmentMutationResult(
        await cloudApi().createEnvironment(
          requireOrganization(context),
          requireProject(context),
          mutation.name,
          mutation.idempotencyKey
        )
      );
    }
    case 'operations list':
      requireListCommand(arguments_);
      return operationsResult(await cloudApi().listOperations(requireOrganization(context)));
    case 'workloads list':
      requireListCommand(arguments_);
      return workloadsResult(
        await cloudApi().listWorkloads(
          requireOrganization(context),
          requireProject(context),
          requireEnvironment(context)
        )
      );
    case 'workloads get':
      requireReadCommand(arguments_, 'workloads get <workload-id>');
      return workloadResult(
        await cloudApi().getWorkload(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'workload ID')
        )
      );
    case 'workloads logs':
      requireArity(positionals, 4, 'workloads logs <workload-id> <revision-id>');
      rejectIdempotencyOption(arguments_);
      rejectFileOption(arguments_);
      rejectExpectedVersionOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      return workloadLogsResult(
        await cloudApi().getWorkloadLogs(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'workload ID'),
          positionalUuid(positionals, 3, 'revision ID'),
          parseLogQuery(arguments_)
        )
      );
    case 'workloads create': {
      const mutation = requireAclMutationCommand(arguments_, 2, 'workloads create');
      const organizationId = requireOrganization(context);
      const projectId = requireProject(context);
      const environmentId = requireEnvironment(context);
      const api = cloudApi();
      const manifest = await readAclDocument(
        mutation.file,
        { label: 'workload ACL', maximumBytes: MAX_WORKLOAD_ACL_BYTES },
        dependencies.readFile
      );
      return workloadDeploymentResult(
        await api.createWorkloadFromAcl(
          organizationId,
          projectId,
          environmentId,
          manifest,
          mutation.idempotencyKey
        )
      );
    }
    case 'workloads update': {
      const mutation = requireAclMutationCommand(arguments_, 3, 'workloads update <workload-id>');
      const organizationId = requireOrganization(context);
      const workloadId = positionalUuid(positionals, 2, 'workload ID');
      const api = cloudApi();
      const manifest = await readAclDocument(
        mutation.file,
        { label: 'workload ACL', maximumBytes: MAX_WORKLOAD_ACL_BYTES },
        dependencies.readFile
      );
      return workloadDeploymentResult(
        await api.updateWorkloadFromAcl(organizationId, workloadId, manifest, mutation.idempotencyKey)
      );
    }
    case 'asset-releases deploy': {
      const mutation = requireAclMutationCommand(
        arguments_,
        4,
        'asset-releases deploy <asset-id> <release-id>'
      );
      const api = cloudApi();
      const manifest = await readAclDocument(
        mutation.file,
        { label: 'workload ACL', maximumBytes: MAX_WORKLOAD_ACL_BYTES },
        dependencies.readFile
      );
      return workloadDeploymentResult(
        await api.deployAgentReleaseFromAcl(
          requireOrganization(context),
          requireProject(context),
          requireEnvironment(context),
          positionalUuid(positionals, 2, 'Asset ID'),
          positionalUuid(positionals, 3, 'Asset release ID'),
          manifest,
          mutation.idempotencyKey
        )
      );
    }
    case 'asset-releases update': {
      const mutation = requireAclMutationCommand(
        arguments_,
        5,
        'asset-releases update <workload-id> <asset-id> <release-id>'
      );
      const api = cloudApi();
      const manifest = await readAclDocument(
        mutation.file,
        { label: 'workload ACL', maximumBytes: MAX_WORKLOAD_ACL_BYTES },
        dependencies.readFile
      );
      return workloadDeploymentResult(
        await api.updateAgentReleaseFromAcl(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Workload ID'),
          positionalUuid(positionals, 3, 'Asset ID'),
          positionalUuid(positionals, 4, 'Asset release ID'),
          manifest,
          mutation.idempotencyKey
        )
      );
    }
    case 'skill-bindings bind': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        5,
        'skill-bindings bind <workload-id> <skill-asset-id> <skill-release-id>'
      );
      return workloadDeploymentResult(
        await cloudApi().bindSkillRelease(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Workload ID'),
          positionalUuid(positionals, 3, 'Skill Asset ID'),
          positionalUuid(positionals, 4, 'Skill Asset release ID'),
          idempotencyKey
        )
      );
    }
    case 'skill-bindings unbind': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        4,
        'skill-bindings unbind <workload-id> <skill-asset-id>'
      );
      return workloadDeploymentResult(
        await cloudApi().unbindSkillRelease(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'Workload ID'),
          positionalUuid(positionals, 3, 'Skill Asset ID'),
          idempotencyKey
        )
      );
    }
    case 'workloads stop': {
      const idempotencyKey = requireMutationCommand(arguments_, 3, 'workloads stop <workload-id>');
      const organizationId = requireOrganization(context);
      const workloadId = positionalUuid(positionals, 2, 'workload ID');
      return stopWorkloadResult(await cloudApi().stopWorkload(organizationId, workloadId, idempotencyKey));
    }
    case 'workloads rollback': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        4,
        'workloads rollback <workload-id> <revision-id>'
      );
      const organizationId = requireOrganization(context);
      const workloadId = positionalUuid(positionals, 2, 'workload ID');
      const revisionId = positionalUuid(positionals, 3, 'revision ID');
      return workloadDeploymentResult(
        await cloudApi().rollbackWorkload(organizationId, workloadId, revisionId, idempotencyKey)
      );
    }
    case 'source-revisions deploy': {
      const mutation = requireAclMutationCommand(
        arguments_,
        3,
        'source-revisions deploy <source-revision-id>'
      );
      const organizationId = requireOrganization(context);
      const projectId = requireProject(context);
      const environmentId = requireEnvironment(context);
      const sourceRevisionId = positionalUuid(positionals, 2, 'source revision ID');
      const api = cloudApi();
      const manifest = await readAclDocument(
        mutation.file,
        { label: 'workload ACL', maximumBytes: MAX_WORKLOAD_ACL_BYTES },
        dependencies.readFile
      );
      return workloadDeploymentResult(
        await api.deploySourceRevisionFromAcl(
          organizationId,
          projectId,
          environmentId,
          sourceRevisionId,
          manifest,
          mutation.idempotencyKey
        )
      );
    }
    case 'deployments get':
      requireReadCommand(arguments_, 'deployments get <deployment-id>');
      return deploymentResult(
        await cloudApi().getDeployment(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'deployment ID')
        )
      );
    case 'deployments cancel': {
      const idempotencyKey = requireMutationCommand(arguments_, 3, 'deployments cancel <deployment-id>');
      const organizationId = requireOrganization(context);
      const deploymentId = positionalUuid(positionals, 2, 'deployment ID');
      return cancelDeploymentResult(
        await cloudApi().cancelDeployment(organizationId, deploymentId, idempotencyKey)
      );
    }
    case 'routes list':
      requireListCommand(arguments_);
      return routesResult(
        await cloudApi().listRoutes(
          requireOrganization(context),
          requireProject(context),
          requireEnvironment(context)
        )
      );
    case 'routes get':
      requireReadCommand(arguments_, 'routes get <route-id>');
      return routeResult(
        await cloudApi().getRoute(requireOrganization(context), positionalUuid(positionals, 2, 'route ID'))
      );
    case 'build-runs list':
      requireListCommand(arguments_);
      return buildRunsResult(
        await cloudApi().listBuildRuns(
          requireOrganization(context),
          requireProject(context),
          requireEnvironment(context)
        )
      );
    case 'build-runs get':
      requireReadCommand(arguments_, 'build-runs get <build-run-id>');
      return buildRunResult(
        await cloudApi().getBuildRun(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'BuildRun ID')
        )
      );
    case 'build-runs evidence':
      requireReadCommand(arguments_, 'build-runs evidence <build-run-id>');
      return buildEvidenceResult(
        await cloudApi().getBuildEvidence(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'BuildRun ID')
        )
      );
    case 'build-runs logs':
      requireArity(positionals, 3, 'build-runs logs <build-run-id>');
      rejectIdempotencyOption(arguments_);
      rejectFileOption(arguments_);
      rejectExpectedVersionOption(arguments_);
      rejectGatewayRolloutOptions(arguments_);
      return buildRunLogsResult(
        await cloudApi().getBuildRunLogs(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'BuildRun ID'),
          parseLogQuery(arguments_)
        )
      );
    case 'build-runs cancel': {
      const idempotencyKey = requireMutationCommand(arguments_, 3, 'build-runs cancel <build-run-id>');
      const organizationId = requireOrganization(context);
      const buildRunId = positionalUuid(positionals, 2, 'BuildRun ID');
      return cancelBuildRunResult(
        await cloudApi().cancelBuildRun(organizationId, buildRunId, idempotencyKey)
      );
    }
    case 'build-runs retry': {
      const idempotencyKey = requireMutationCommand(arguments_, 3, 'build-runs retry <build-run-id>');
      const organizationId = requireOrganization(context);
      const buildRunId = positionalUuid(positionals, 2, 'BuildRun ID');
      return retryBuildRunResult(await cloudApi().retryBuildRun(organizationId, buildRunId, idempotencyKey));
    }
    default:
      throw usageError('unsupported command; run a3s-cloud --help');
  }
}

function rejectMisplacedProjectAttributionOptions(command: string, arguments_: ParsedArguments): void {
  if (
    command !== 'project-attribution update' &&
    (arguments_.costAttributionCode !== undefined || arguments_.projectAttributionLabels.length > 0)
  ) {
    throw usageError('--cost-attribution-code and --label are valid only for project-attribution update');
  }
}

function parseProjectAttributionLabels(values: readonly string[]): Record<string, string> {
  const labels: Record<string, string> = {};
  for (const pair of values) {
    const separator = pair.indexOf('=');
    if (separator < 1 || separator === pair.length - 1) {
      throw usageError('--label must use a non-empty key=value pair');
    }
    const key = pair.slice(0, separator);
    if (Object.hasOwn(labels, key)) {
      throw usageError(`project attribution label ${JSON.stringify(key)} is duplicated`);
    }
    labels[key] = pair.slice(separator + 1);
  }
  return labels;
}

function requireNamedMutationCommand(
  arguments_: ParsedArguments,
  usage: string
): { idempotencyKey: string; name: string } {
  const idempotencyKey = requireMutationCommand(arguments_, 3, usage);
  const name = positionalResourceName(arguments_.positionals, 2);
  return { idempotencyKey, name };
}

function parseLogQuery(arguments_: ParsedArguments): CloudLogQuery {
  const query: CloudLogQuery = {};
  if (arguments_.cursor !== undefined) {
    const cursor = arguments_.cursor;
    if (cursor.length === 0 || cursor.length > 1_024 || hasUnsafeControl(cursor)) {
      throw usageError('log cursor is invalid');
    }
    query.cursor = cursor;
  }
  if (arguments_.limit !== undefined) {
    if (!/^[0-9]+$/.test(arguments_.limit)) {
      throw usageError('log limit must be an integer');
    }
    const limit = Number(arguments_.limit);
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 256) {
      throw usageError('log limit must be between 1 and 256');
    }
    query.limit = limit;
  }
  if (arguments_.stream !== undefined) {
    if (arguments_.stream !== 'stdout' && arguments_.stream !== 'stderr') {
      throw usageError('log stream must be stdout or stderr');
    }
    query.stream = arguments_.stream;
  }
  return query;
}
