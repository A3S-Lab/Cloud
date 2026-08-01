import { describe, expect, test } from 'bun:test';
import { createElement } from 'react';
import { ReactFlowProvider } from '@xyflow/react';
import TestRenderer, { act, type ReactTestInstance } from 'react-test-renderer';
import type { StudioNode } from '../graph';
import type { NodeKind, RuntimeEvidence } from '../types';
import { Inspector } from './Inspector';
import { NodeIcon } from './NodeIcon';
import { WorkflowCardNode } from './WorkflowCardNode';

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
  .IS_REACT_ACT_ENVIRONMENT = true;

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

function studioNode(kind: NodeKind = 'template'): StudioNode {
  return {
    id: `${kind}-node`,
    type: 'workflow',
    position: { x: 10, y: 20 },
    data: {
      kind,
      label: `${kind} label`,
      config: { template: 'Hello {{name}}' },
      runtime: {
        provider: 'production',
        pool: 'cpu',
        cpuMillis: 500,
        memoryBytes: 256 * 1024 * 1024,
        timeoutMs: 120_000,
        secrets: [],
      },
      executionState: 'succeeded',
      runtimeUnit: 'workflow/run/node/template/execute',
    },
  };
}

function textContent(node: ReactTestInstance): string {
  return node.children
    .map((child) => (typeof child === 'string' ? child : textContent(child)))
    .join('');
}

function buttonWithText(
  renderer: TestRenderer.ReactTestRenderer,
  label: string,
): ReactTestInstance {
  return renderer.root
    .findAllByType('button')
    .find((item) => textContent(item).trim().startsWith(label))!;
}

describe('Studio node components', () => {
  test('renders a distinct accessible SVG for every node kind', async () => {
    let renderer: TestRenderer.ReactTestRenderer;
    await act(async () => {
      renderer = TestRenderer.create(
        <div>{kinds.map((kind) => <NodeIcon key={kind} kind={kind} size={24} />)}</div>,
      );
    });

    const icons = renderer!.root.findAllByType('svg');
    expect(icons).toHaveLength(kinds.length);
    for (const icon of icons) {
      expect(icon.props.width).toBe(24);
      expect(icon.props.height).toBe(24);
      expect(icon.props['aria-hidden']).toBe(true);
    }
    expect(icons.map((icon) => icon.findAllByType('path').length)).toEqual([
      1, 1, 2, 1, 1, 1, 1, 1, 2, 1,
    ]);
  });

  test('renders node boundaries, runtime placement and deduplicated router handles', async () => {
    const router = studioNode('router');
    router.data.config = {
      routes: [{ route: 'approved' }, { route: '' }, { route: 'approved' }, {}],
      default: 'fallback',
    };
    let renderer: TestRenderer.ReactTestRenderer;
    await act(async () => {
      renderer = TestRenderer.create(
        <ReactFlowProvider>
          {createElement(WorkflowCardNode, {
            id: router.id,
            data: router.data,
            selected: true,
          } as never)}
        </ReactFlowProvider>,
      );
    });

    const card = renderer!.root.findByProps({
      'data-testid': 'workflow-node-router-node',
    });
    expect(card.props.className).toContain('selected');
    expect(textContent(card)).toContain('production / cpu');
    expect(textContent(renderer!.root.findByProps({ 'aria-label': 'Router outputs' }))).toContain(
      'approvedfallback',
    );
    expect(renderer!.root.findAllByProps({ title: 'succeeded' })).toHaveLength(1);

    router.data.config = null;
    await act(async () => {
      renderer!.update(
        <ReactFlowProvider>
          {createElement(WorkflowCardNode, {
            id: router.id,
            data: router.data,
            selected: false,
          } as never)}
        </ReactFlowProvider>,
      );
    });
    expect(textContent(renderer!.root.findByProps({ 'aria-label': 'Router outputs' }))).toContain(
      'default',
    );
  });

  test('renders every category and the correct start/output handle boundaries', async () => {
    let renderer: TestRenderer.ReactTestRenderer;
    await act(async () => {
      renderer = TestRenderer.create(
        <ReactFlowProvider>
          <div>
            {kinds
              .filter((kind) => kind !== 'router')
              .map((kind) => {
                const value = studioNode(kind);
                value.data.runtime = { secrets: [] };
                value.data.executionState = undefined;
                return createElement(WorkflowCardNode, {
                  key: kind,
                  id: value.id,
                  data: value.data,
                  selected: false,
                } as never);
              })}
          </div>
        </ReactFlowProvider>,
      );
    });

    expect(renderer!.root.findAll((node) => node.props.className === 'node-title')).toHaveLength(
      9,
    );
    expect(renderer!.root.findByProps({ 'data-testid': 'workflow-node-start-node' })).toBeDefined();
    expect(renderer!.root.findByProps({ 'data-testid': 'workflow-node-output-node' })).toBeDefined();
  });
});

describe('Runtime policy inspector', () => {
  test('edits identity, placement, resources and typed JSON configuration', async () => {
    const changes: Array<{ id: string; data: StudioNode['data'] }> = [];
    const deleted: string[] = [];
    let closed = 0;
    const node = studioNode();
    const evidence: RuntimeEvidence = {
      executionId: 'execution-1',
      runId: 'run-1',
      stepId: 'node:template:execute',
      attempt: 2,
      nodeId: node.id,
      providerId: 'production-cpu',
      runtimePool: 'cpu',
      unitId: 'workflow/run-1/node/template/execute',
      generation: 2,
      specDigest: `sha256:${'a'.repeat(64)}`,
      state: 'succeeded',
      observation: {},
    };
    let renderer: TestRenderer.ReactTestRenderer;
    await act(async () => {
      renderer = TestRenderer.create(
        <Inspector
          node={node}
          evidence={evidence}
          onChange={(id, data) => changes.push({ id, data })}
          onDelete={(id) => deleted.push(id)}
          onClose={() => { closed += 1; }}
        />,
      );
    });

    const change = async (props: Record<string, unknown>, value: string) => {
      await act(async () => {
        renderer!.root.findByProps(props).props.onChange({ target: { value } });
      });
    };
    await change({ 'aria-label': 'Display name' }, 'Renamed');
    await change({ 'aria-label': 'Node configuration JSON' }, '{broken');
    await act(async () => {
      renderer!.root.findByProps({ className: 'secondary-button full' }).props.onClick();
    });
    expect(renderer!.root.findAllByProps({ className: 'field-error' })).toHaveLength(1);

    await change({ 'aria-label': 'Node configuration JSON' }, '{"template":"updated"}');
    await act(async () => {
      renderer!.root.findByProps({ className: 'secondary-button full' }).props.onClick();
      buttonWithText(renderer!, 'RUNTIME').props.onClick();
    });
    await change({ 'aria-label': 'Runtime provider' }, 'edge');
    await change({ 'aria-label': 'Runtime provider' }, '');
    await change({ 'aria-label': 'Runtime pool' }, 'gpu');
    await change({ 'aria-label': 'Runtime isolation' }, 'sandbox');
    await change({ 'aria-label': 'Runtime network' }, 'none');
    await change({ placeholder: '500' }, '750');
    await change({ placeholder: '500' }, '');
    await change({ placeholder: '256' }, '512');
    await change({ placeholder: '256' }, '');
    await change({ placeholder: '120000' }, '45000');

    expect(changes.some((entry) => entry.data.label === 'Renamed')).toBe(true);
    expect(changes.some((entry) => entry.data.runtime.provider === 'edge')).toBe(true);
    expect(changes.some((entry) => entry.data.runtime.provider === undefined)).toBe(true);
    expect(changes.some((entry) => entry.data.runtime.pool === 'gpu')).toBe(true);
    expect(changes.some((entry) => entry.data.runtime.isolation === 'sandbox')).toBe(true);
    expect(changes.some((entry) => entry.data.runtime.network === 'none')).toBe(true);
    expect(changes.some((entry) => entry.data.runtime.cpuMillis === 750)).toBe(true);
    expect(changes.some((entry) => entry.data.runtime.memoryBytes === 512 * 1024 * 1024)).toBe(true);
    expect(changes.some((entry) => entry.data.runtime.timeoutMs === 45_000)).toBe(true);
    expect(changes.some((entry) => JSON.stringify(entry.data.config).includes('updated'))).toBe(
      true,
    );

    await act(async () => {
      buttonWithText(renderer!, 'EVIDENCE').props.onClick();
    });
    expect(renderer!.root.findAllByProps({ 'data-testid': 'runtime-evidence' })).toHaveLength(1);
    expect(textContent(renderer!.root.findByProps({ 'data-testid': 'runtime-evidence' }))).toContain(
      'production-cpu',
    );

    await act(async () => {
      renderer!.root.findByProps({ 'aria-label': 'Close node inspector' }).props.onClick();
      renderer!.root.findByProps({ className: 'danger-button' }).props.onClick();
    });
    expect(closed).toBe(1);
    expect(deleted).toEqual([node.id]);
  });

  test('resets when selection changes and protects boundary nodes from deletion', async () => {
    let renderer: TestRenderer.ReactTestRenderer;
    await act(async () => {
      renderer = TestRenderer.create(
        <Inspector node={undefined} onChange={() => {}} onDelete={() => {}} />,
      );
    });
    expect(textContent(renderer!.root.findByProps({ 'aria-label': 'Node inspector' }))).toContain(
      'No node selected',
    );

    const start = studioNode('start');
    start.data.config = { input: true };
    await act(async () => {
      renderer!.update(<Inspector node={start} onChange={() => {}} onDelete={() => {}} />);
    });
    expect(renderer!.root.findByProps({ 'aria-label': 'Node configuration JSON' }).props.value).toContain(
      'input',
    );
    expect(renderer!.root.findAllByProps({ className: 'danger-button' })).toHaveLength(0);
    await act(async () => {
      buttonWithText(renderer!, 'EVIDENCE').props.onClick();
    });
    expect(textContent(renderer!.root.findByProps({ className: 'evidence-empty' }))).toContain(
      'No Runtime evidence yet',
    );
    await act(async () => {
      buttonWithText(renderer!, 'RUNTIME').props.onClick();
    });
    expect(renderer!.root.findByProps({ 'aria-label': 'Runtime network' }).props.value).toBe(
      'none',
    );

    const llm = studioNode('llm');
    llm.data.runtime.network = undefined;
    await act(async () => {
      renderer!.update(<Inspector node={llm} onChange={() => {}} onDelete={() => {}} />);
    });
    expect(renderer!.root.findByProps({ 'aria-label': 'Node configuration JSON' })).toBeDefined();
    await act(async () => {
      buttonWithText(renderer!, 'RUNTIME').props.onClick();
    });
    expect(renderer!.root.findByProps({ 'aria-label': 'Runtime network' }).props.value).toBe(
      'outbound',
    );
  });
});
