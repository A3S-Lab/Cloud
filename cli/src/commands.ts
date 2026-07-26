import {
  CloudApi,
  type CloudFetch,
  type CloudLogQuery,
  isValidIdempotencyKey,
  MAX_WORKLOAD_ACL_BYTES,
} from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import type { CloudContext } from './context';
import {
  hasUnsafeControl,
  parseUuid,
  publicContext,
  requireEnvironment,
  requireOrganization,
  requireProject,
  requireToken,
} from './context';
import { usageError } from './errors';
import {
  buildEvidenceResult,
  buildRunLogsResult,
  buildRunResult,
  buildRunsResult,
  cancelBuildRunResult,
  cancelDeploymentResult,
  contextResult,
  deploymentResult,
  environmentsResult,
  nodesResult,
  operationsResult,
  organizationsResult,
  projectsResult,
  retryBuildRunResult,
  routeResult,
  routesResult,
  stopWorkloadResult,
  type CommandResult,
  workloadLogsResult,
  workloadDeploymentResult,
  workloadResult,
  workloadsResult,
} from './results';

export interface CommandDependencies {
  fetch?: CloudFetch;
  readFile?: (path: string) => Promise<Uint8Array>;
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
  if (command === 'context show') {
    requireArity(positionals, 2, 'context show');
    rejectLogOptions(arguments_);
    rejectIdempotencyOption(arguments_);
    rejectFileOption(arguments_);
    return contextResult(publicContext(context));
  }

  let api: CloudApi | undefined;
  const cloudApi = (): CloudApi => {
    api ??= new CloudApi(requireToken(context), context.baseUrl, {
      fetch: dependencies.fetch,
      requestTimeoutMs: context.timeoutMs,
    });
    return api;
  };
  switch (command) {
    case 'organizations list':
      requireListCommand(arguments_);
      return organizationsResult(await cloudApi().listOrganizations());
    case 'projects list':
      requireListCommand(arguments_);
      return projectsResult(await cloudApi().listProjects(requireOrganization(context)));
    case 'environments list':
      requireListCommand(arguments_);
      return environmentsResult(
        await cloudApi().listEnvironments(requireOrganization(context), requireProject(context))
      );
    case 'nodes list':
      requireListCommand(arguments_);
      return nodesResult(await cloudApi().listNodes(requireOrganization(context)));
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
      const manifest = await readAclManifest(mutation.file, dependencies.readFile);
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
      const manifest = await readAclManifest(mutation.file, dependencies.readFile);
      return workloadDeploymentResult(
        await api.updateWorkloadFromAcl(organizationId, workloadId, manifest, mutation.idempotencyKey)
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
      const manifest = await readAclManifest(mutation.file, dependencies.readFile);
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

function requireListCommand(arguments_: ParsedArguments): void {
  requireArity(arguments_.positionals, 2, `${arguments_.positionals[0]} list`);
  rejectLogOptions(arguments_);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
}

function requireReadCommand(arguments_: ParsedArguments, usage: string): void {
  requireArity(arguments_.positionals, 3, usage);
  rejectLogOptions(arguments_);
  rejectIdempotencyOption(arguments_);
  rejectFileOption(arguments_);
}

function requireMutationCommand(arguments_: ParsedArguments, arity: number, usage: string): string {
  requireArity(arguments_.positionals, arity, usage);
  rejectLogOptions(arguments_);
  const key = arguments_.idempotencyKey;
  if (key === undefined) {
    throw usageError('--idempotency-key is required for mutation commands');
  }
  if (!isValidIdempotencyKey(key)) {
    throw usageError('idempotency key is invalid');
  }
  rejectFileOption(arguments_);
  return key;
}

function requireAclMutationCommand(
  arguments_: ParsedArguments,
  arity: number,
  usage: string
): { idempotencyKey: string; file: string } {
  requireArity(arguments_.positionals, arity, usage);
  rejectLogOptions(arguments_);
  const idempotencyKey = arguments_.idempotencyKey;
  if (idempotencyKey === undefined) {
    throw usageError('--idempotency-key is required for mutation commands');
  }
  if (!isValidIdempotencyKey(idempotencyKey)) {
    throw usageError('idempotency key is invalid');
  }
  const file = arguments_.file;
  if (file === undefined) {
    throw usageError('--file is required for ACL desired-state mutations');
  }
  if (file.length > 4_096 || /[\0\r\n]/.test(file)) {
    throw usageError('ACL file path is invalid');
  }
  return { idempotencyKey, file };
}

function requireArity(positionals: readonly string[], expected: number, usage: string): void {
  if (positionals.length !== expected) {
    throw usageError(`usage: a3s-cloud ${usage}`);
  }
}

function positionalUuid(positionals: readonly string[], index: number, label: string): string {
  const value = positionals[index];
  if (!value) {
    throw usageError(`${label} is required`);
  }
  return parseUuid(value, label);
}

function rejectLogOptions(arguments_: ParsedArguments): void {
  if (arguments_.cursor !== undefined || arguments_.limit !== undefined || arguments_.stream !== undefined) {
    throw usageError('cursor, limit, and stream options are valid only for log commands');
  }
}

function rejectIdempotencyOption(arguments_: ParsedArguments): void {
  if (arguments_.idempotencyKey !== undefined) {
    throw usageError('--idempotency-key is valid only for mutation commands');
  }
}

function rejectFileOption(arguments_: ParsedArguments): void {
  if (arguments_.file !== undefined) {
    throw usageError('--file is valid only for ACL desired-state mutations');
  }
}

async function readAclManifest(
  path: string,
  readFile: (path: string) => Promise<Uint8Array> = readLocalFile
): Promise<string> {
  let bytes: Uint8Array;
  try {
    bytes = await readFile(path);
  } catch {
    throw usageError('unable to read the A3S ACL file');
  }
  if (bytes.byteLength < 1 || bytes.byteLength > MAX_WORKLOAD_ACL_BYTES) {
    throw usageError(`workload ACL must contain between 1 and ${MAX_WORKLOAD_ACL_BYTES} UTF-8 bytes`);
  }
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    throw usageError('workload ACL must be valid UTF-8');
  }
}

async function readLocalFile(path: string): Promise<Uint8Array> {
  return Bun.file(path).bytes();
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
