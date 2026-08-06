import type {
  BuildEvidence,
  BuildRun,
  BuildRunLogsPage,
  CancelBuildRunResult,
  CancelDeploymentResult,
  CloudDiagnostics,
  Deployment,
  DomainClaim,
  DomainClaimMutationResult,
  Environment,
  EnvironmentMutationResult,
  GatewayScope,
  GatewayScopeMutationResult,
  McpCredential,
  McpCredentialDeliveryResult,
  McpCredentialMutationResult,
  Node,
  Operation,
  Organization,
  OrganizationMutationResult,
  Project,
  ProjectMutationResult,
  RetryBuildRunResult,
  Route,
  RoutePublicationResult,
  StopWorkloadResult,
  Workload,
  WorkloadDeploymentResult,
  WorkloadLogRecord,
  WorkloadLogsPage,
} from '@a3s/cloud-client';
import type { PublicCloudContext } from './context';
import { ExitCode, type ExitCodeValue } from './errors';
import { renderTable, sanitizeCell, type TableColumn } from './output';

export interface CommandResult {
  json: unknown;
  table: string;
  exitCode?: ExitCodeValue;
}

export function contextResult(context: PublicCloudContext): CommandResult {
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

export function diagnosticsResult(diagnostics: CloudDiagnostics): CommandResult {
  const rows = [
    {
      component: 'platform',
      status: 'up',
      details: `${diagnostics.platform.name}@${diagnostics.platform.version} role=${diagnostics.platform.role}`,
    },
    {
      component: 'liveness',
      status: diagnostics.liveness.status,
      details: healthChecks(diagnostics.liveness.checks),
    },
    {
      component: 'readiness',
      status: diagnostics.readiness.status,
      details: healthChecks(diagnostics.readiness.checks),
    },
  ];
  const healthy = diagnostics.liveness.status === 'up' && diagnostics.readiness.status === 'up';
  return {
    json: diagnostics,
    table: renderTable(rows, [
      { header: 'COMPONENT', value: (row) => row.component },
      { header: 'STATUS', value: (row) => row.status },
      { header: 'DETAILS', value: (row) => row.details },
    ]),
    exitCode: healthy ? ExitCode.Success : ExitCode.Unhealthy,
  };
}

function healthChecks(checks: CloudDiagnostics['readiness']['checks']): string {
  const entries = Object.entries(checks).map(([name, result]) => `${name}=${result.status}`);
  return entries.length === 0 ? '-' : entries.join(', ');
}

const ORGANIZATION_COLUMNS: readonly TableColumn<Organization>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'NAME', value: (row) => row.name },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'CREATED AT', value: (row) => row.createdAt },
];

export function organizationsResult(rows: Organization[]): CommandResult {
  return listResult(rows, ORGANIZATION_COLUMNS);
}

export function organizationMutationResult(row: OrganizationMutationResult): CommandResult {
  return singleResult(row, [
    ...ORGANIZATION_COLUMNS,
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
}

const PROJECT_COLUMNS: readonly TableColumn<Project>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'NAME', value: (row) => row.name },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'CREATED AT', value: (row) => row.createdAt },
];

export function projectsResult(rows: Project[]): CommandResult {
  return listResult(rows, PROJECT_COLUMNS);
}

export function projectMutationResult(row: ProjectMutationResult): CommandResult {
  return singleResult(row, [...PROJECT_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]);
}

const ENVIRONMENT_COLUMNS: readonly TableColumn<Environment>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'NAME', value: (row) => row.name },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'CREATED AT', value: (row) => row.createdAt },
];

export function environmentsResult(rows: Environment[]): CommandResult {
  return listResult(rows, ENVIRONMENT_COLUMNS);
}

export function environmentMutationResult(row: EnvironmentMutationResult): CommandResult {
  return singleResult(row, [
    ...ENVIRONMENT_COLUMNS,
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
}

export function nodesResult(rows: Node[]): CommandResult {
  return listResult(rows, [
    { header: 'ID', value: (row) => row.id },
    { header: 'NAME', value: (row) => row.name },
    { header: 'STATE', value: (row) => row.state },
    { header: 'AVAILABILITY', value: (row) => row.availability },
    { header: 'PROVIDER', value: (row) => row.runtimeProviderId },
    { header: 'LAST OBSERVED', value: (row) => row.lastObservedAt },
  ]);
}

export function nodeMutationResult(row: Node): CommandResult {
  return singleResult(row, [
    { header: 'ID', value: (value) => value.id },
    { header: 'NAME', value: (value) => value.name },
    { header: 'STATE', value: (value) => value.state },
    { header: 'AVAILABILITY', value: (value) => value.availability },
    { header: 'VERSION', value: (value) => value.aggregateVersion },
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
}

export function operationsResult(rows: Operation[]): CommandResult {
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

export function workloadsResult(rows: Workload[]): CommandResult {
  return listResult(rows, WORKLOAD_COLUMNS);
}

export function workloadResult(row: Workload): CommandResult {
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

export function deploymentResult(row: Deployment): CommandResult {
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

export function routesResult(rows: Route[]): CommandResult {
  return listResult(rows, ROUTE_COLUMNS);
}

export function routeResult(row: Route): CommandResult {
  return singleResult(row, ROUTE_COLUMNS);
}

const DOMAIN_CLAIM_COLUMNS: readonly TableColumn<DomainClaim>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'PATTERN', value: (row) => row.pattern },
  { header: 'STATE', value: (row) => row.state },
  { header: 'CHALLENGE NAME', value: (row) => row.challengeDnsName },
  { header: 'CHALLENGE VALUE', value: (row) => row.challengeValue },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
  { header: 'FAILURE', value: (row) => row.failure },
];

export function domainClaimsResult(rows: DomainClaim[]): CommandResult {
  return listResult(rows, DOMAIN_CLAIM_COLUMNS);
}

export function domainClaimResult(row: DomainClaim): CommandResult {
  return singleResult(row, DOMAIN_CLAIM_COLUMNS);
}

export function domainClaimMutationResult(row: DomainClaimMutationResult): CommandResult {
  return singleResult(row, [
    ...DOMAIN_CLAIM_COLUMNS,
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
}

const GATEWAY_SCOPE_COLUMNS: readonly TableColumn<GatewayScope>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'PRIMARY NODE', value: (row) => row.nodeId },
  { header: 'MEMBERS', value: (row) => row.memberNodeIds.join(',') },
  { header: 'GENERATION', value: (row) => row.membershipGeneration },
  { header: 'MIN READY', value: (row) => row.minReady },
  { header: 'MAX UNAVAILABLE', value: (row) => row.maxUnavailable },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
];

export function gatewayScopesResult(rows: GatewayScope[]): CommandResult {
  return listResult(rows, GATEWAY_SCOPE_COLUMNS);
}

export function gatewayScopeResult(row: GatewayScope): CommandResult {
  return singleResult(row, GATEWAY_SCOPE_COLUMNS);
}

export function gatewayScopeMutationResult(row: GatewayScopeMutationResult): CommandResult {
  return singleResult(row, [
    ...GATEWAY_SCOPE_COLUMNS,
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
}

const MCP_CREDENTIAL_COLUMNS: readonly TableColumn<McpCredential>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'PREFIX', value: (row) => row.prefix },
  { header: 'STATE', value: (row) => row.state },
  { header: 'GENERATION', value: (row) => row.generation },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'EXPIRES AT', value: (row) => row.expiresAt },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
];

export function mcpCredentialsResult(rows: McpCredential[]): CommandResult {
  return listResult(rows, MCP_CREDENTIAL_COLUMNS);
}

export function mcpCredentialResult(row: McpCredential): CommandResult {
  return singleResult(row, MCP_CREDENTIAL_COLUMNS);
}

export function mcpCredentialDeliveryResult(row: McpCredentialDeliveryResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ID', value: (value) => value.credential.id },
        { header: 'PREFIX', value: (value) => value.credential.prefix },
        { header: 'GENERATION', value: (value) => value.credential.generation },
        { header: 'VERSION', value: (value) => value.credential.aggregateVersion },
        { header: 'BEARER CREDENTIAL', value: (value) => value.bearerCredential },
        { header: 'DELIVERY EXPIRES', value: (value) => value.deliveryExpiresAt },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

export function mcpCredentialMutationResult(row: McpCredentialMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ID', value: (value) => value.credential.id },
        { header: 'PREFIX', value: (value) => value.credential.prefix },
        { header: 'STATE', value: (value) => value.credential.state },
        { header: 'GENERATION', value: (value) => value.credential.generation },
        { header: 'VERSION', value: (value) => value.credential.aggregateVersion },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

export function routePublicationResult(row: RoutePublicationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ROUTE', value: (value) => value.route.id },
        { header: 'HOST', value: (value) => `${value.route.hostname}${value.route.pathPrefix}` },
        { header: 'STATE', value: (value) => value.route.state },
        { header: 'CERTIFICATE', value: (value) => value.certificate.id },
        { header: 'CERTIFICATE STATE', value: (value) => value.certificate.state },
        { header: 'REPLAYED', value: (value) => value.replayed },
        { header: 'COMMAND REPLAYED', value: (value) => value.commandReplayed },
      ]
    ),
  };
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

export function buildRunsResult(rows: BuildRun[]): CommandResult {
  return listResult(rows, BUILD_RUN_COLUMNS);
}

export function buildRunResult(row: BuildRun): CommandResult {
  return singleResult(row, BUILD_RUN_COLUMNS);
}

export function buildEvidenceResult(row: BuildEvidence): CommandResult {
  return singleResult(row, [
    { header: 'BUILD RUN', value: (value) => value.buildRunId },
    { header: 'REPOSITORY', value: (value) => value.repository },
    { header: 'COMMIT', value: (value) => value.commitSha },
    { header: 'ARTIFACT', value: (value) => value.artifact.digest },
    { header: 'VERIFICATION', value: (value) => value.verificationState },
    { header: 'ATTESTED AT', value: (value) => value.attestedAt },
  ]);
}

export function workloadLogsResult(page: WorkloadLogsPage): CommandResult {
  return logPageResult(page, page.records);
}

export function buildRunLogsResult(page: BuildRunLogsPage): CommandResult {
  return logPageResult(page, page.records);
}

export function stopWorkloadResult(row: StopWorkloadResult): CommandResult {
  return singleResult(row, [
    { header: 'WORKLOAD', value: (value) => value.workloadId },
    { header: 'OPERATION', value: (value) => value.operationId },
    { header: 'DESIRED', value: (value) => value.desiredState },
    { header: 'REQUESTED AT', value: (value) => value.requestedAt },
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
}

export function workloadDeploymentResult(row: WorkloadDeploymentResult): CommandResult {
  return singleResult(row, [
    { header: 'WORKLOAD', value: (value) => value.workloadId },
    { header: 'REVISION', value: (value) => value.revisionId },
    { header: 'DEPLOYMENT', value: (value) => value.deploymentId },
    { header: 'OPERATION', value: (value) => value.operationId },
    { header: 'GENERATION', value: (value) => value.generation },
    { header: 'STATUS', value: (value) => value.status },
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
}

export function cancelDeploymentResult(row: CancelDeploymentResult): CommandResult {
  return singleResult(row, [
    { header: 'DEPLOYMENT', value: (value) => value.deploymentId },
    { header: 'OPERATION', value: (value) => value.operationId },
    { header: 'STATUS', value: (value) => value.status },
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
}

export function cancelBuildRunResult(row: CancelBuildRunResult): CommandResult {
  return singleResult(row, [
    { header: 'BUILD RUN', value: (value) => value.buildRunId },
    { header: 'OPERATION', value: (value) => value.operationId },
    { header: 'STATUS', value: (value) => value.status },
    { header: 'CANCEL REQUESTED', value: (value) => value.cancellationRequestedAt },
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
}

export function retryBuildRunResult(row: RetryBuildRunResult): CommandResult {
  return singleResult(row, [
    { header: 'BUILD RUN', value: (value) => value.buildRunId },
    { header: 'RETRY OF', value: (value) => value.retryOfBuildRunId },
    { header: 'OPERATION', value: (value) => value.operationId },
    { header: 'ATTEMPT', value: (value) => value.attempt },
    { header: 'STATUS', value: (value) => value.status },
    { header: 'REPLAYED', value: (value) => value.replayed },
  ]);
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
