import { describe, expect, it } from 'vitest';
import type { SearchResult } from '../../types/api';
import { parseCloudLocation, selectionFromSearchResult } from './cloud-location';

const ORGANIZATION_ID = '019c0000-0000-7000-8000-000000000001';
const PROJECT_ID = '019c0000-0000-7000-8000-000000000002';
const ENVIRONMENT_ID = '019c0000-0000-7000-8000-000000000003';
const WORKLOAD_ID = '019c0000-0000-7000-8000-000000000004';
const RESOURCE_ID = '019c0000-0000-7000-8000-000000000005';

describe('Cloud search locations', () => {
  it('parses an exact server-generated contextual resource location', () => {
    expect(
      parseCloudLocation(
        `#/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/workloads/${WORKLOAD_ID}`
      )
    ).toEqual({
      organizationId: ORGANIZATION_ID,
      projectId: PROJECT_ID,
      environmentId: ENVIRONMENT_ID,
      resourceKind: 'workload',
      resourceId: WORKLOAD_ID,
    });
  });

  it.each([
    ['', null],
    ['#/projects/not-an-organization', null],
    [`#/organizations/not-a-uuid`, null],
    [`#/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/extra`, null],
    [`#/organizations/${ORGANIZATION_ID}/unknown/${RESOURCE_ID}`, null],
  ])('rejects a malformed or unsupported location %#', (hash, expected) => {
    expect(parseCloudLocation(hash)).toBe(expected);
  });

  it('selects the related workload for route and deployment results', () => {
    const selection = selectionFromSearchResult(
      searchResult({
        kind: 'route',
        id: RESOURCE_ID,
        workloadId: WORKLOAD_ID,
        href: `#/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/routes/${RESOURCE_ID}`,
      })
    );

    expect(selection).toEqual(
      expect.objectContaining({
        organizationId: ORGANIZATION_ID,
        projectId: PROJECT_ID,
        environmentId: ENVIRONMENT_ID,
        workloadId: WORKLOAD_ID,
        buildRunId: null,
        openOperations: false,
        href: expect.stringContaining(`/routes/${RESOURCE_ID}`),
      })
    );
  });

  it('selects BuildRun results and opens the operation drawer for operations', () => {
    const build = selectionFromSearchResult(
      searchResult({
        kind: 'build_run',
        id: RESOURCE_ID,
        href: `#/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/build-runs/${RESOURCE_ID}`,
      })
    );
    const operation = selectionFromSearchResult(
      searchResult({
        kind: 'operation',
        id: RESOURCE_ID,
        projectId: null,
        environmentId: null,
        href: `#/organizations/${ORGANIZATION_ID}/operations/${RESOURCE_ID}`,
      })
    );

    expect(build.buildRunId).toBe(RESOURCE_ID);
    expect(build.workloadId).toBeNull();
    expect(operation.openOperations).toBe(true);
  });

  it('ignores a mismatched server href while preserving authorized result context', () => {
    const selection = selectionFromSearchResult(
      searchResult({
        kind: 'workload',
        id: WORKLOAD_ID,
        workloadId: WORKLOAD_ID,
        href: `#/organizations/019c0000-0000-7000-8000-000000000099/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/workloads/${WORKLOAD_ID}`,
      })
    );

    expect(selection.organizationId).toBe(ORGANIZATION_ID);
    expect(selection.workloadId).toBe(WORKLOAD_ID);
    expect(selection.href).toBeNull();
  });
});

function searchResult(overrides: Partial<SearchResult>): SearchResult {
  return {
    organizationId: ORGANIZATION_ID,
    projectId: PROJECT_ID,
    environmentId: ENVIRONMENT_ID,
    workloadId: null,
    kind: 'workload',
    id: WORKLOAD_ID,
    title: 'Cloud worker',
    description: 'Workload · desired running',
    state: 'running',
    href: `#/organizations/${ORGANIZATION_ID}/projects/${PROJECT_ID}/environments/${ENVIRONMENT_ID}/workloads/${WORKLOAD_ID}`,
    updatedAt: '2026-07-27T01:00:00.000Z',
    ...overrides,
  };
}
