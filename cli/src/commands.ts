import {
  type BuildEvidence,
  type BuildRun,
  type BuildRunLogsPage,
  CloudApi,
  type CloudFetch,
  type CloudLogQuery,
  type Deployment,
  type Environment,
  type Node,
  type Operation,
  type Organization,
  type Project,
  type Route,
  type Workload,
  type WorkloadLogRecord,
  type WorkloadLogsPage,
} from '@a3s/cloud-client';
import type { ParsedArguments } from './arguments';
import type { CloudContext, PublicCloudContext } from './context';
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
import { renderTable, sanitizeCell, type TableColumn } from './output';

export interface CommandResult {
  json: unknown;
  table: string;
}

export async function executeCommand(
  arguments_: ParsedArguments,
  context: CloudContext,
  fetcher?: CloudFetch
): Promise<CommandResult> {
  const { positionals } = arguments_;
  if (positionals.length < 2) {
    throw usageError('a command and action are required; run a3s-cloud --help');
  }
  const command = `${positionals[0]} ${positionals[1]}`;
  if (command === 'context show') {
    requireArity(positionals, 2, 'context show');
    rejectLogOptions(arguments_);
    return contextResult(publicContext(context));
  }

  let api: CloudApi | undefined;
  const cloudApi = (): CloudApi => {
    api ??= new CloudApi(requireToken(context), context.baseUrl, {
      fetch: fetcher,
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
      return workloadLogsResult(
        await cloudApi().getWorkloadLogs(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'workload ID'),
          positionalUuid(positionals, 3, 'revision ID'),
          parseLogQuery(arguments_)
        )
      );
    case 'deployments get':
      requireReadCommand(arguments_, 'deployments get <deployment-id>');
      return deploymentResult(
        await cloudApi().getDeployment(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'deployment ID')
        )
      );
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
      return buildRunLogsResult(
        await cloudApi().getBuildRunLogs(
          requireOrganization(context),
          positionalUuid(positionals, 2, 'BuildRun ID'),
          parseLogQuery(arguments_)
        )
      );
    default:
      throw usageError('unsupported command; run a3s-cloud --help');
  }
}

function requireListCommand(arguments_: ParsedArguments): void {
  requireArity(arguments_.positionals, 2, `${arguments_.positionals[0]} list`);
  rejectLogOptions(arguments_);
}

function requireReadCommand(arguments_: ParsedArguments, usage: string): void {
  requireArity(arguments_.positionals, 3, usage);
  rejectLogOptions(arguments_);
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

function contextResult(context: PublicCloudContext): CommandResult {
  const rows = [
    { key: 'URL', value: context.url },
    { key: 'Organization', value: context.organizationId ?? '' },
    { key: 'Project', value: context.projectId ?? '' },
    { key: 'Environment', value: context.environmentId ?? '' },
    { key: 'Output', value: context.output },
    { key: 'Timeout (ms)', value: context.timeoutMs },
    { key: 'Token configured', value: context.tokenConfigured ? 'yes' : 'no' },
  ];
  return {
    json: context,
    table: renderTable(rows, [
      { header: 'CONTEXT', value: (row) => row.key },
      { header: 'VALUE', value: (row) => row.value },
    ]),
  };
}

function organizationsResult(rows: Organization[]): CommandResult {
  return listResult(rows, [
    { header: 'ID', value: (row) => row.id },
    { header: 'NAME', value: (row) => row.name },
    { header: 'VERSION', value: (row) => row.aggregateVersion },
    { header: 'CREATED AT', value: (row) => row.createdAt },
  ]);
}

function projectsResult(rows: Project[]): CommandResult {
  return listResult(rows, [
    { header: 'ID', value: (row) => row.id },
    { header: 'NAME', value: (row) => row.name },
    { header: 'VERSION', value: (row) => row.aggregateVersion },
    { header: 'CREATED AT', value: (row) => row.createdAt },
  ]);
}

function environmentsResult(rows: Environment[]): CommandResult {
  return listResult(rows, [
    { header: 'ID', value: (row) => row.id },
    { header: 'NAME', value: (row) => row.name },
    { header: 'VERSION', value: (row) => row.aggregateVersion },
    { header: 'CREATED AT', value: (row) => row.createdAt },
  ]);
}

function nodesResult(rows: Node[]): CommandResult {
  return listResult(rows, [
    { header: 'ID', value: (row) => row.id },
    { header: 'NAME', value: (row) => row.name },
    { header: 'STATE', value: (row) => row.state },
    { header: 'AVAILABILITY', value: (row) => row.availability },
    { header: 'PROVIDER', value: (row) => row.runtimeProviderId },
    { header: 'LAST OBSERVED', value: (row) => row.lastObservedAt },
  ]);
}

function operationsResult(rows: Operation[]): CommandResult {
  return listResult(rows, [
    { header: 'ID', value: (row) => row.id },
    { header: 'SUBJECT', value: (row) => `${row.subjectKind}/${row.subjectId}` },
    { header: 'WORKFLOW', value: (row) => `${row.workflowName}@${row.workflowVersion}` },
    { header: 'STATUS', value: (row) => row.status },
    { header: 'UPDATED AT', value: (row) => row.updatedAt },
    { header: 'ERROR', value: (row) => row.error },
  ]);
}

const WORKLOAD_COLUMNS: readonly TableColumn<Workload>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'NAME', value: (row) => row.name },
  { header: 'DESIRED', value: (row) => row.desiredState },
  { header: 'ACTIVE REVISION', value: (row) => row.activeRevision?.generation },
  { header: 'DEPLOYMENTS', value: (row) => row.deployments.length },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
];

function workloadsResult(rows: Workload[]): CommandResult {
  return listResult(rows, WORKLOAD_COLUMNS);
}

function workloadResult(row: Workload): CommandResult {
  return singleResult(row, WORKLOAD_COLUMNS);
}

const DEPLOYMENT_COLUMNS: readonly TableColumn<Deployment>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'WORKLOAD', value: (row) => row.workloadId },
  { header: 'REVISION', value: (row) => row.revision.generation },
  { header: 'STATUS', value: (row) => row.status },
  { header: 'NODE', value: (row) => row.nodeId },
  { header: 'HEALTH', value: (row) => row.observedRuntime?.healthState },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
  { header: 'FAILURE', value: (row) => row.failure },
];

function deploymentResult(row: Deployment): CommandResult {
  return singleResult(row, DEPLOYMENT_COLUMNS);
}

const ROUTE_COLUMNS: readonly TableColumn<Route>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'HOST', value: (row) => `${row.hostname}${row.pathPrefix}` },
  { header: 'STATE', value: (row) => row.state },
  { header: 'WORKLOAD', value: (row) => row.workloadId },
  { header: 'REVISION', value: (row) => row.workloadRevisionId },
  { header: 'GATEWAY', value: (row) => row.gatewayNodeId },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
  { header: 'FAILURE', value: (row) => row.failure },
];

function routesResult(rows: Route[]): CommandResult {
  return listResult(rows, ROUTE_COLUMNS);
}

function routeResult(row: Route): CommandResult {
  return singleResult(row, ROUTE_COLUMNS);
}

const BUILD_RUN_COLUMNS: readonly TableColumn<BuildRun>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'STATUS', value: (row) => row.status },
  { header: 'ATTEMPT', value: (row) => row.attempt },
  { header: 'SOURCE REVISION', value: (row) => row.sourceRevisionId },
  { header: 'ARTIFACT', value: (row) => row.publishedArtifact?.digest },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
  { header: 'FAILURE', value: (row) => row.failure },
];

function buildRunsResult(rows: BuildRun[]): CommandResult {
  return listResult(rows, BUILD_RUN_COLUMNS);
}

function buildRunResult(row: BuildRun): CommandResult {
  return singleResult(row, BUILD_RUN_COLUMNS);
}

function buildEvidenceResult(row: BuildEvidence): CommandResult {
  return singleResult(row, [
    { header: 'BUILD RUN', value: (value) => value.buildRunId },
    { header: 'REPOSITORY', value: (value) => value.repository },
    { header: 'COMMIT', value: (value) => value.commitSha },
    { header: 'ARTIFACT', value: (value) => value.artifact.digest },
    { header: 'VERIFICATION', value: (value) => value.verificationState },
    { header: 'ATTESTED AT', value: (value) => value.attestedAt },
  ]);
}

function workloadLogsResult(page: WorkloadLogsPage): CommandResult {
  return logPageResult(page, page.records);
}

function buildRunLogsResult(page: BuildRunLogsPage): CommandResult {
  return logPageResult(page, page.records);
}

function logPageResult(page: WorkloadLogsPage | BuildRunLogsPage, rows: WorkloadLogRecord[]): CommandResult {
  const table = renderTable(rows, [
    { header: 'SEQUENCE', value: (row) => row.sequence },
    { header: 'STREAM', value: (row) => row.stream },
    { header: 'KIND', value: (row) => row.kind },
    { header: 'OBSERVED MS', value: (row) => row.observedAtMs },
    { header: 'DATA / GAP', value: logRecordValue },
  ]);
  return {
    json: page,
    table: page.nextCursor ? `${table}Next cursor: ${sanitizeCell(page.nextCursor)}\n` : table,
  };
}

function logRecordValue(row: WorkloadLogRecord): string | null {
  if (row.kind === 'data') {
    return row.data;
  }
  const range = row.fromSequence === null ? '' : ` ${row.fromSequence}-${row.throughSequence}`;
  return `${row.gapReason ?? 'unknown'}${range}`;
}

function singleResult<Row>(row: Row, columns: readonly TableColumn<Row>[]): CommandResult {
  return {
    json: row,
    table: renderTable([row], columns),
  };
}

function listResult<Row>(rows: Row[], columns: readonly TableColumn<Row>[]): CommandResult {
  return {
    json: rows,
    table: renderTable(rows, columns),
  };
}
