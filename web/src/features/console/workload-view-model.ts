import type { Operation, ServiceTemplate, Workload, WorkloadRevision } from '../../types/api';

export interface TemplateChange {
  path: string;
  before: string;
  after: string;
}

export interface ParsedServiceTemplateDraft {
  template: ServiceTemplate | null;
  error: string | null;
}

export function diffTemplates(before: ServiceTemplate, after: ServiceTemplate): TemplateChange[] {
  const changes: TemplateChange[] = [];
  visitDifference(before, after, '', changes);
  return changes;
}

export function eligibleRollbackRevisions(workload: Workload | undefined): WorkloadRevision[] {
  if (!workload || workload.desiredState !== 'running' || !workload.activeRevision) {
    return [];
  }
  const active = workload.activeRevision;
  const eligible = new Map<string, WorkloadRevision>();
  for (const deployment of workload.deployments) {
    if (
      deployment.revision.id !== active.id &&
      deployment.revision.generation < active.generation &&
      deployment.status === 'active' &&
      deployment.activatedAt
    ) {
      eligible.set(deployment.revision.id, deployment.revision);
    }
  }
  return [...eligible.values()].sort((left, right) => right.generation - left.generation);
}

export function isWorkloadReadyForReplacement(workload: Workload | undefined): boolean {
  if (
    !workload ||
    workload.desiredState !== 'running' ||
    !workload.desiredRevision ||
    !workload.activeRevision ||
    workload.desiredRevision.id !== workload.activeRevision.id
  ) {
    return false;
  }
  return workload.deployments.some(
    (deployment) => deployment.revision.id === workload.activeRevision?.id && deployment.status === 'active'
  );
}

export function parseServiceTemplateDraft(value: string): ParsedServiceTemplateDraft {
  let parsed: unknown;
  try {
    parsed = JSON.parse(value) as unknown;
  } catch {
    return {
      template: null,
      error: 'The template is not valid JSON.',
    };
  }
  if (!isServiceTemplate(parsed)) {
    return {
      template: null,
      error: 'The template must include artifact, process, secrets, resources, ports, and health.',
    };
  }
  return { template: parsed, error: null };
}

export function isTerminalOperation(operation: Operation): boolean {
  return (
    operation.status === 'succeeded' || operation.status === 'failed' || operation.status === 'cancelled'
  );
}

export function visibleOperations(operations: Operation[], dismissed: ReadonlySet<string>): Operation[] {
  return operations.filter((operation) => !dismissed.has(operation.id) || !isTerminalOperation(operation));
}

function visitDifference(before: unknown, after: unknown, path: string, changes: TemplateChange[]): void {
  if (Object.is(before, after)) {
    return;
  }
  if (Array.isArray(before) || Array.isArray(after)) {
    const beforeArray = Array.isArray(before) ? before : [];
    const afterArray = Array.isArray(after) ? after : [];
    const length = Math.max(beforeArray.length, afterArray.length);
    for (let index = 0; index < length; index += 1) {
      visitDifference(beforeArray[index], afterArray[index], `${path}[${index}]`, changes);
    }
    return;
  }
  if (isRecord(before) || isRecord(after)) {
    const beforeRecord = isRecord(before) ? before : {};
    const afterRecord = isRecord(after) ? after : {};
    const keys = [...new Set([...Object.keys(beforeRecord), ...Object.keys(afterRecord)])].sort();
    for (const key of keys) {
      visitDifference(beforeRecord[key], afterRecord[key], path ? `${path}.${key}` : key, changes);
    }
    return;
  }
  changes.push({
    path,
    before: formatValue(before),
    after: formatValue(after),
  });
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function isServiceTemplate(value: unknown): value is ServiceTemplate {
  if (!isRecord(value)) return false;
  return (
    isArtifact(value.artifact) &&
    isProcess(value.process) &&
    Array.isArray(value.secrets) &&
    value.secrets.every(isSecretBinding) &&
    isResources(value.resources) &&
    Array.isArray(value.ports) &&
    value.ports.every(isPort) &&
    isHealth(value.health)
  );
}

function isArtifact(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.uri === 'string' &&
    (value.expectedDigest === null || typeof value.expectedDigest === 'string')
  );
}

function isProcess(value: unknown): boolean {
  return (
    isRecord(value) &&
    isStringArray(value.command) &&
    isStringArray(value.args) &&
    (value.workingDirectory === null || typeof value.workingDirectory === 'string') &&
    isStringRecord(value.environment)
  );
}

function isSecretBinding(value: unknown): boolean {
  if (
    !isRecord(value) ||
    typeof value.name !== 'string' ||
    typeof value.secretId !== 'string' ||
    !isFiniteNumber(value.version) ||
    !isRecord(value.target) ||
    typeof value.target.kind !== 'string'
  ) {
    return false;
  }
  if (value.target.kind === 'environment') {
    return typeof value.target.variable === 'string';
  }
  if (value.target.kind === 'file') {
    return typeof value.target.path === 'string' && isFiniteNumber(value.target.mode);
  }
  return value.target.kind === 'registry_credential';
}

function isResources(value: unknown): boolean {
  return (
    isRecord(value) &&
    isFiniteNumber(value.cpuMillis) &&
    isFiniteNumber(value.memoryBytes) &&
    isFiniteNumber(value.pids) &&
    (value.ephemeralStorageBytes === null || isFiniteNumber(value.ephemeralStorageBytes))
  );
}

function isPort(value: unknown): boolean {
  return isRecord(value) && typeof value.name === 'string' && isFiniteNumber(value.containerPort);
}

function isHealth(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.portName === 'string' &&
    typeof value.path === 'string' &&
    isFiniteNumber(value.intervalMs) &&
    isFiniteNumber(value.timeoutMs) &&
    isFiniteNumber(value.healthyThreshold) &&
    isFiniteNumber(value.unhealthyThreshold) &&
    isFiniteNumber(value.stabilizationWindowMs)
  );
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item) => typeof item === 'string');
}

function isStringRecord(value: unknown): value is Record<string, string> {
  return isRecord(value) && Object.values(value).every((item) => typeof item === 'string');
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function formatValue(value: unknown): string {
  if (value === undefined) return '—';
  if (typeof value === 'string') return value;
  if (value === null || typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return JSON.stringify(value);
}
