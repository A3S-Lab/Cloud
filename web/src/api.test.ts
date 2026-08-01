import { afterEach, describe, expect, test } from 'bun:test';
import { api } from './api';
import type { Workflow } from './types';

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
});

function response(body: unknown, status = 200) {
  return new Response(body === undefined ? undefined : JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

function workflow(): Workflow {
  return {
    id: 'workflow-1',
    name: 'Demo',
    description: 'Description',
    version: 3,
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
    nodes: [],
    edges: [],
  };
}

describe('Workflow API client', () => {
  test('uses the versioned API root and JSON headers', async () => {
    const calls: Array<{ input: string; init?: RequestInit }> = [];
    globalThis.fetch = (async (input, init) => {
      calls.push({ input: String(input), init });
      return response([workflow()]);
    }) as typeof fetch;

    await expect(api.listWorkflows()).resolves.toHaveLength(1);
    expect(calls).toHaveLength(1);
    expect(calls[0].input).toBe('/api/v1/workflows');
    expect(calls[0].init?.headers).toEqual({ 'content-type': 'application/json' });
  });

  test('updates only the mutable workflow wire fields', async () => {
    const source = workflow();
    let captured: { input: string; init?: RequestInit } | undefined;
    globalThis.fetch = (async (input, init) => {
      captured = { input: String(input), init };
      return response(source);
    }) as typeof fetch;

    await api.updateWorkflow(source);
    expect(captured?.input).toBe('/api/v1/workflows/workflow-1');
    expect(captured?.init?.method).toBe('PUT');
    expect(JSON.parse(String(captured?.init?.body))).toEqual({
      version: 3,
      name: 'Demo',
      description: 'Description',
      nodes: [],
      edges: [],
    });
  });

  test('routes runs, evidence and approvals to their precise endpoints', async () => {
    const calls: Array<{ input: string; init?: RequestInit }> = [];
    globalThis.fetch = (async (input, init) => {
      calls.push({ input: String(input), init });
      return response({ runId: 'run-1' });
    }) as typeof fetch;

    await api.startRun('workflow-1', { value: 7 });
    await api.getRun('run-1');
    await api.listRuntimeEvidence('run-1');
    await api.approve('run-1', 'approval-1', { approved: true });

    expect(calls.map((call) => call.input)).toEqual([
      '/api/v1/workflows/workflow-1/runs',
      '/api/v1/runs/run-1',
      '/api/v1/runs/run-1/node-executions',
      '/api/v1/runs/run-1/approvals/approval-1',
    ]);
    expect(calls[0].init?.method).toBe('POST');
    expect(calls[3].init?.method).toBe('POST');
    expect(JSON.parse(String(calls[3].init?.body))).toEqual({
      payload: { approved: true },
    });
  });

  test('surfaces response bodies and status fallbacks for HTTP failures', async () => {
    globalThis.fetch = (async () =>
      new Response('validation failed', { status: 422 })) as unknown as typeof fetch;
    await expect(api.listNodeTypes()).rejects.toThrow('validation failed');

    globalThis.fetch = (async () =>
      new Response('', { status: 503 })) as unknown as typeof fetch;
    await expect(api.listNodeTypes()).rejects.toThrow('Request failed with 503');
  });

  test('accepts successful no-content responses', async () => {
    globalThis.fetch = (async () =>
      new Response(undefined, { status: 204 })) as unknown as typeof fetch;

    await expect(api.listNodeTypes()).resolves.toBeUndefined();
  });
});
