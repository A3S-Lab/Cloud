export type SearchResourceKind =
  | 'project'
  | 'environment'
  | 'node'
  | 'workload'
  | 'deployment'
  | 'route'
  | 'domain_claim'
  | 'gateway_scope'
  | 'build_run'
  | 'source_revision'
  | 'secret'
  | 'operation';

export interface SearchResult {
  organizationId: string;
  projectId: string | null;
  environmentId: string | null;
  workloadId: string | null;
  kind: SearchResourceKind;
  id: string;
  title: string;
  description: string;
  state: string | null;
  href: string;
  updatedAt: string;
}

export const DEFAULT_SEARCH_LIMIT = 20;
export const MAX_SEARCH_RESULTS = 50;
const MAX_SEARCH_QUERY_CHARACTERS = 128;

export function validateSearchRequest(query: string, limit: number): string {
  const normalized = query.trim();
  if (
    normalized.length === 0 ||
    Array.from(normalized).length > MAX_SEARCH_QUERY_CHARACTERS ||
    /[\0\r\n]/.test(normalized)
  ) {
    throw new RangeError(`search query must contain 1 to ${MAX_SEARCH_QUERY_CHARACTERS} safe characters`);
  }
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_SEARCH_RESULTS) {
    throw new RangeError(`search result limit must be between 1 and ${MAX_SEARCH_RESULTS}`);
  }
  return normalized;
}
