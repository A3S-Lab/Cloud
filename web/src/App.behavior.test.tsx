import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { createElement } from 'react';
import TestRenderer, { act, type ReactTestInstance } from 'react-test-renderer';
import { App } from './App';
import type {
  NodeDescriptor,
  NodeKind,
  RuntimeEvidence,
  Workflow,
  WorkflowRun,
} from './types';

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
  .IS_REACT_ACT_ENVIRONMENT = true;
const browserGlobal = globalThis as typeof globalThis & {
  window: Window & typeof globalThis;
  addEventListener: Window['addEventListener'];
  removeEventListener: Window['removeEventListener'];
  requestAnimationFrame: typeof requestAnimationFrame;
  cancelAnimationFrame: typeof cancelAnimationFrame;
};
browserGlobal.window = browserGlobal as unknown as Window & typeof globalThis;
browserGlobal.addEventListener ??= () => {};
browserGlobal.removeEventListener ??= () => {};
browserGlobal.requestAnimationFrame ??= (callback) =>
  setTimeout(() => callback(performance.now()), 0) as unknown as number;
browserGlobal.cancelAnimationFrame ??= (handle) => clearTimeout(handle);

const originalFetch = globalThis.fetch;
let renderers: TestRenderer.ReactTestRenderer[] = [];

beforeEach(() => {
  renderers = [];
});

afterEach(async () => {
  await act(async () => {
    for (const renderer of renderers) renderer.unmount();
  });
  globalThis.fetch = originalFetch;
});

const kinds: NodeKind[] = [
  'start',
  'template',
  'llm',
  'agent',
  'tool',
  'router',
  'memory',
  'http',
  'approval',
  'output',
];

function descriptor(kind: NodeKind): NodeDescriptor {
  return {
    kind,
    label: `${kind} node`,
    description: `Execute the ${kind} node through a dedicated Runtime provider.`,
    defaultConfig: kind === 'template' ? { template: 'Hello {{name}}' } : {},
  };
}

function workflow(id: string, name: string): Workflow {
  return {
    id,
    name,
    description: `${name} workflow`,
    version: 1,
    createdAt: '2026-08-01T00:00:00Z',
    updatedAt: '2026-08-01T00:00:00Z',
    nodes: [
      {
        id: 'start',
        type: 'start',
        position: { x: 0, y: 0 },
        data: { label: 'Start', config: {}, runtime: { secrets: [] } },
      },
      {
        id: 'approval',
        type: 'approval',
        position: { x: 240, y: 0 },
        data: { label: 'Approval', config: {}, runtime: { secrets: [] } },
      },
      {
        id: 'output',
        type: 'output',
        position: { x: 480, y: 0 },
        data: { label: 'Output', config: {}, runtime: { secrets: [] } },
      },
    ],
    edges: [
      { id: 'start-approval', source: 'start', target: 'approval' },
      { id: 'approval-output', source: 'approval', target: 'output' },
    ],
  };
}

function run(status: WorkflowRun['status'], hooks: WorkflowRun['hooks'] = {}): WorkflowRun {
  return {
    run_id: 'run-behavior-1',
    status,
    output: status === 'completed' ? { message: 'done' } : null,
    error: status === 'failed' ? 'approval required' : null,
    steps: {},
    hooks,
  };
}

function runtimeEvidence(): RuntimeEvidence[] {
  return [
    {
      executionId: 'execution-start',
      runId: 'run-behavior-1',
      stepId: 'node:start:execute',
      attempt: 1,
      nodeId: 'start',
      providerId: 'production-start',
      runtimePool: 'start',
      unitId: 'workflow/run-behavior-1/node/start/execute',
      generation: 1,
      specDigest: `sha256:${'a'.repeat(64)}`,
      state: 'succeeded',
      observation: {},
    },
  ];
}

function response(body: unknown, status = 200): Response {
  return new Response(body === undefined ? undefined : JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

async function flush(times = 2): Promise<void> {
  for (let index = 0; index < times; index += 1) {
    await act(async () => {
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
    });
  }
}

async function renderApp(): Promise<TestRenderer.ReactTestRenderer> {
  let renderer: TestRenderer.ReactTestRenderer;
  await act(async () => {
    renderer = TestRenderer.create(createElement(App));
  });
  renderers.push(renderer!);
  await flush();
  return renderer!;
}

function button(renderer: TestRenderer.ReactTestRenderer, testId: string): ReactTestInstance {
  return renderer.root.findByProps({ 'data-testid': testId });
}

function textContent(node: ReactTestInstance): string {
  return node.children
    .map((child) => (typeof child === 'string' ? child : textContent(child)))
    .join('');
}

describe('Studio workflow behavior', () => {
  test('authors, saves, runs and approves a Runtime-backed graph', async () => {
    const primary = workflow('workflow-1', 'Primary');
    const secondary = workflow('workflow-2', 'Secondary');
    const calls: Array<{ path: string; method: string; body?: unknown }> = [];
    let pollCount = 0;
    let approvalCount = 0;

    globalThis.fetch = (async (input, init) => {
      const path = String(input);
      const method = init?.method ?? 'GET';
      calls.push({
        path,
        method,
        body: init?.body ? JSON.parse(String(init.body)) : undefined,
      });
      if (path === '/api/v1/workflows' && method === 'GET') {
        return response([primary, secondary]);
      }
      if (path === '/api/v1/node-types') return response(kinds.map(descriptor));
      if (path.startsWith('/api/v1/workflows/') && method === 'PUT') {
        const body = JSON.parse(String(init?.body));
        return response({
          ...primary,
          ...body,
          version: primary.version + 1,
          updatedAt: '2026-08-01T00:01:00Z',
        });
      }
      if (path === '/api/v1/workflows/workflow-1/runs' && method === 'POST') {
        return response(
          run('running', {
            approval: {
              hook_id: 'approval',
              status: 'active',
              metadata: { subject: 'Deploy production' },
            },
          }),
        );
      }
      if (path === '/api/v1/runs/run-behavior-1' && method === 'GET') {
        pollCount += 1;
        return response(
          pollCount === 1
            ? run('failed', {
                approval: {
                  hook_id: 'approval',
                  status: 'active',
                  metadata: { subject: 'Deploy production' },
                },
              })
            : run('completed'),
        );
      }
      if (path === '/api/v1/runs/run-behavior-1/node-executions') {
        return response(runtimeEvidence());
      }
      if (path === '/api/v1/runs/run-behavior-1/approvals/approval') {
        approvalCount += 1;
        return approvalCount === 1
          ? new Response('approval service unavailable', { status: 503 })
          : response(run('running'));
      }
      return new Response('unexpected request', { status: 500 });
    }) as typeof fetch;

    const renderer = await renderApp();
    expect(renderer.root.findByProps({ 'aria-label': 'Node library' })).toBeDefined();
    expect(renderer.root.findByProps({ 'data-testid': 'workflow-canvas' })).toBeDefined();
    expect(renderer.root.findByProps({ 'aria-label': 'Execution console' })).toBeDefined();
    expect(renderer.root.findByProps({ 'aria-label': 'Node inspector' })).toBeDefined();
    expect(renderer.root.findAllByType('nav')).toHaveLength(0);
    expect(renderer.root.findAllByProps({ 'aria-label': 'Primary navigation' })).toHaveLength(0);
    expect(renderer.root.findAllByProps({ 'aria-label': 'Account' })).toHaveLength(0);
    expect(JSON.stringify(renderer.toJSON())).not.toContain('Runtime providers');
    expect(JSON.stringify(renderer.toJSON())).not.toContain('Search nodes');
    expect(renderer.root.findByProps({ 'aria-label': 'Select workflow' }).props.value).toBe(
      'workflow-1',
    );
    expect(button(renderer, 'add-node-template')).toBeDefined();

    await act(async () => {
      button(renderer, 'add-node-template').props.onClick();
    });
    expect(renderer.root.findAllByProps({ 'data-testid': 'node-inspector' })).toHaveLength(1);
    expect(textContent(renderer.root.findByProps({ className: 'toast' }))).toContain('node added');

    await act(async () => {
      renderer.root.findByProps({ 'aria-label': 'Display name' }).props.onChange({
        target: { value: 'Prompt renderer' },
      });
    });
    expect(textContent(renderer.root.findByProps({ 'data-testid': 'node-inspector' }))).toContain(
      'Prompt renderer',
    );

    const flow = renderer.root.find(
      (node) => typeof node.props.onConnect === 'function' && Array.isArray(node.props.nodes),
    );
    await act(async () => {
      flow.props.onConnect({ source: null, target: 'output' });
      flow.props.onConnect({ source: 'start', target: 'output', sourceHandle: null });
      flow.props.onNodeClick({}, { id: 'start' });
    });
    expect(textContent(renderer.root.findByProps({ 'data-testid': 'node-inspector' }))).toContain(
      'Start',
    );
    await act(async () => {
      flow.props.onPaneClick();
    });
    expect(textContent(renderer.root.findByProps({ 'aria-label': 'Node inspector' }))).toContain(
      'No node selected',
    );

    await act(async () => {
      button(renderer, 'add-node-start').props.onClick();
    });
    expect(textContent(renderer.root.findByProps({ className: 'toast' }))).toContain(
      'already has a start',
    );
    await act(async () => {
      renderer.root.findByProps({ className: 'toast' }).props.onClick();
    });
    expect(renderer.root.findAllByProps({ className: 'toast' })).toHaveLength(0);

    await act(async () => {
      renderer.root.findByProps({ 'aria-label': 'Select workflow' }).props.onChange({
        target: { value: 'missing' },
      });
      renderer.root.findByProps({ 'aria-label': 'Select workflow' }).props.onChange({
        target: { value: 'workflow-2' },
      });
    });
    expect(renderer.root.findByProps({ 'aria-label': 'Select workflow' }).props.value).toBe(
      'workflow-2',
    );
    await act(async () => {
      renderer.root.findByProps({ 'aria-label': 'Select workflow' }).props.onChange({
        target: { value: 'workflow-1' },
      });
    });

    await act(async () => {
      button(renderer, 'save-workflow').props.onClick();
    });
    await flush();
    expect(calls.some((call) => call.method === 'PUT')).toBe(true);

    await act(async () => {
      renderer.root.findByProps({ 'data-testid': 'run-input' }).props.onChange({
        target: { value: '{"name":"Ada"}' },
      });
      button(renderer, 'run-workflow').props.onClick();
    });
    await flush(4);
    expect(pollCount).toBe(1);
    expect(textContent(renderer.root.findByProps({ 'data-testid': 'run-output' }))).toContain(
      'approval required',
    );
    expect(textContent(renderer.root.findByProps({ 'data-testid': 'execution-track' }))).toContain(
      'production-start',
    );

    const approval = () => renderer.root.findByProps({ className: 'approval-button' });
    await act(async () => {
      approval().props.onClick();
    });
    await flush();
    expect(textContent(renderer.root.findByProps({ className: 'toast error' }))).toContain(
      'approval service unavailable',
    );
    await act(async () => {
      renderer.root.findByProps({ className: 'toast error' }).props.onClick();
      approval().props.onClick();
    });
    await flush(4);
    expect(pollCount).toBe(2);
    expect(textContent(renderer.root.findByProps({ 'data-testid': 'run-output' }))).toContain(
      'done',
    );

    const minimap = renderer.root.find((node) => typeof node.props.nodeColor === 'function');
    expect(minimap.props.nodeColor({ data: { kind: 'llm' } })).toBe('#7a5cff');
    expect(minimap.props.nodeColor({ data: { kind: 'tool' } })).toBe('#21b7a8');
    expect(minimap.props.nodeColor({ data: { kind: 'approval' } })).toBe('#e2a93b');
    expect(minimap.props.nodeColor({ data: { kind: 'output' } })).toBe('#2587f5');

    const dock = renderer.root.findByProps({ className: 'dock-toggle' });
    await act(async () => dock.props.onClick());
    expect(renderer.root.findAllByProps({ className: 'dock-body' })).toHaveLength(0);
    await act(async () => renderer.root.findByProps({ className: 'dock-toggle' }).props.onClick());
    expect(renderer.root.findAllByProps({ className: 'dock-body' })).toHaveLength(1);
  });

  test('surfaces initialization, invalid input, save and poll failures', async () => {
    globalThis.fetch = (async () => {
      throw 'offline';
    }) as unknown as typeof fetch;
    const failedInitialization = await renderApp();
    expect(textContent(failedInitialization.root.findByProps({ className: 'toast error' }))).toBe(
      'offline',
    );
    await act(async () => failedInitialization.unmount());
    renderers = renderers.filter((item) => item !== failedInitialization);

    const source = workflow('workflow-errors', 'Errors');
    let failSave = false;
    let failPoll = false;
    globalThis.fetch = (async (input, init) => {
      const path = String(input);
      if (path === '/api/v1/workflows') return response([source]);
      if (path === '/api/v1/node-types') return response(kinds.map(descriptor));
      if (path === '/api/v1/workflows/workflow-errors' && init?.method === 'PUT') {
        if (failSave) return new Response('save rejected', { status: 409 });
        return response({ ...source, version: 2 });
      }
      if (path === '/api/v1/workflows/workflow-errors/runs') return response(run('running'));
      if (path === '/api/v1/runs/run-behavior-1') {
        return failPoll ? new Response('poll failed', { status: 503 }) : response(run('completed'));
      }
      if (path.endsWith('/node-executions')) return response([]);
      return new Response('unexpected request', { status: 500 });
    }) as typeof fetch;
    const renderer = await renderApp();

    await act(async () => {
      renderer.root.findByProps({ 'data-testid': 'run-input' }).props.onChange({
        target: { value: '{invalid' },
      });
    });
    await act(async () => {
      button(renderer, 'run-workflow').props.onClick();
    });
    await flush();
    expect(textContent(renderer.root.findByProps({ className: 'toast error' }))).toContain(
      'JSON',
    );

    await act(async () => renderer.root.findByProps({ className: 'toast error' }).props.onClick());
    failSave = true;
    await act(async () => button(renderer, 'save-workflow').props.onClick());
    await flush();
    expect(textContent(renderer.root.findByProps({ className: 'toast error' }))).toContain(
      'save rejected',
    );

    failSave = false;
    failPoll = true;
    await act(async () => {
      renderer.root.findByProps({ className: 'toast error' }).props.onClick();
      renderer.root.findByProps({ 'data-testid': 'run-input' }).props.onChange({
        target: { value: '{}' },
      });
    });
    await act(async () => {
      button(renderer, 'run-workflow').props.onClick();
    });
    await flush(4);
    expect(textContent(renderer.root.findByProps({ className: 'toast error' }))).toContain(
      'poll failed',
    );
  });
});
