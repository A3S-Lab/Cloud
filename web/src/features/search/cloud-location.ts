import type { SearchResourceKind, SearchResult } from '../../types/api';

export interface CloudLocation {
  organizationId: string;
  projectId: string | null;
  environmentId: string | null;
  resourceKind: SearchResourceKind | null;
  resourceId: string | null;
}

export interface SearchSelection {
  organizationId: string;
  projectId: string | null;
  environmentId: string | null;
  workloadId: string | null;
  buildRunId: string | null;
  openOperations: boolean;
  href: string | null;
}

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const RESOURCE_KIND_BY_SEGMENT: Readonly<Record<string, SearchResourceKind>> = {
  nodes: 'node',
  workloads: 'workload',
  deployments: 'deployment',
  routes: 'route',
  'domain-claims': 'domain_claim',
  'gateway-scopes': 'gateway_scope',
  'build-runs': 'build_run',
  'source-revisions': 'source_revision',
  secrets: 'secret',
  operations: 'operation',
};

export function parseCloudLocation(hash: string): CloudLocation | null {
  if (!hash.startsWith('#/')) {
    return null;
  }
  const segments = hash.slice(2).split('/');
  if (segments[0] !== 'organizations' || !isUuid(segments[1])) {
    return null;
  }

  const organizationId = segments[1].toLowerCase();
  let projectId: string | null = null;
  let environmentId: string | null = null;
  let index = 2;

  if (segments[index] === 'projects') {
    if (!isUuid(segments[index + 1])) {
      return null;
    }
    projectId = segments[index + 1].toLowerCase();
    index += 2;
    if (segments[index] === 'environments') {
      if (!isUuid(segments[index + 1])) {
        return null;
      }
      environmentId = segments[index + 1].toLowerCase();
      index += 2;
    }
  }

  if (index === segments.length) {
    return { organizationId, projectId, environmentId, resourceKind: null, resourceId: null };
  }
  if (segments.length - index !== 2 || !isUuid(segments[index + 1])) {
    return null;
  }
  const resourceKind = RESOURCE_KIND_BY_SEGMENT[segments[index]];
  if (!resourceKind) {
    return null;
  }
  return {
    organizationId,
    projectId,
    environmentId,
    resourceKind,
    resourceId: segments[index + 1].toLowerCase(),
  };
}

export function selectionFromSearchResult(result: SearchResult): SearchSelection {
  const location = parseCloudLocation(result.href);
  return {
    organizationId: result.organizationId,
    projectId: result.projectId,
    environmentId: result.environmentId,
    workloadId: result.workloadId ?? (result.kind === 'workload' ? result.id : null),
    buildRunId: result.kind === 'build_run' ? result.id : null,
    openOperations: result.kind === 'operation',
    href: location && locationMatchesResult(location, result) ? result.href : null,
  };
}

function locationMatchesResult(location: CloudLocation, result: SearchResult): boolean {
  if (
    location.organizationId !== result.organizationId ||
    location.projectId !== result.projectId ||
    location.environmentId !== result.environmentId
  ) {
    return false;
  }
  if (result.kind === 'project') {
    return (
      location.resourceKind === null && location.projectId === result.id && location.environmentId === null
    );
  }
  if (result.kind === 'environment') {
    return location.resourceKind === null && location.environmentId === result.id;
  }
  return location.resourceKind === result.kind && location.resourceId === result.id;
}

function isUuid(value: string | undefined): value is string {
  return value !== undefined && UUID_PATTERN.test(value);
}
