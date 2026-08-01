import { describe, expect, test } from 'bun:test';
import type { Edge } from '@xyflow/react';
import { mergeCanvas, toCanvasEdges, toCanvasNodes } from './graph';
import type { RuntimeEvidence, Workflow } from './types';

const workflow: Workflow = {
  id: 'demo',
  name: 'Demo',
  description: '',
  version: 1,
  createdAt: '2026-01-01T00:00:00Z',
  updatedAt: '2026-01-01T00:00:00Z',
  nodes: [
    {
      id: 'start',
      type: 'start',
      position: { x: 0, y: 0 },
      data: { label: 'Start', config: {}, runtime: { secrets: [] } },
    },
  ],
  edges: [],
};

describe('workflow canvas conversion', () => {
  test('preserves the backend wire contract', () => {
    const nodes = toCanvasNodes(workflow, []);
    nodes[0].position = { x: 120, y: 80 };
    const merged = mergeCanvas(workflow, nodes, toCanvasEdges([]));
    expect(merged.nodes[0]).toEqual({
      ...workflow.nodes[0],
      position: { x: 120, y: 80 },
    });
  });

  test('decorates canvas nodes with matching runtime evidence only', () => {
    const evidence: RuntimeEvidence[] = [
      {
        executionId: 'execution-1',
        runId: 'run-1',
        stepId: 'step-1',
        nodeId: 'start',
        attempt: 1,
        providerId: 'production-start-pool',
        runtimePool: 'start-pool',
        unitId: 'unit-1',
        generation: 2,
        specDigest: 'sha256:spec',
        state: 'succeeded',
        observation: {},
      },
    ];

    const [node] = toCanvasNodes(workflow, evidence);
    expect(node.data.executionState).toBe('succeeded');
    expect(node.data.runtimeUnit).toBe('unit-1');
    expect(workflow.nodes[0].data).not.toHaveProperty('executionState');
  });

  test('adds React Flow presentation without changing edge routing data', () => {
    const [edge] = toCanvasEdges([
      {
        id: 'route-a',
        source: 'router',
        target: 'output',
        sourceHandle: 'approved',
      },
    ]);

    expect(edge).toMatchObject({
      id: 'route-a',
      source: 'router',
      target: 'output',
      sourceHandle: 'approved',
      type: 'smoothstep',
      animated: false,
      style: { stroke: '#98a2b3', strokeWidth: 1.5 },
    });
  });

  test('serializes newly added nodes and strips transient execution state', () => {
    const nodes = toCanvasNodes(workflow, []);
    nodes.push({
      id: 'output',
      type: 'workflow',
      position: { x: 300, y: 0 },
      data: {
        kind: 'output',
        label: 'Output',
        config: {},
        runtime: { provider: 'production', pool: 'output-pool', secrets: [] },
        executionState: 'running',
        runtimeUnit: 'transient-unit',
      },
    });
    const edges: Edge[] = [
      {
        id: 'edge',
        source: 'start',
        target: 'output',
        sourceHandle: null,
      },
    ];

    const merged = mergeCanvas(workflow, nodes, edges);
    expect(merged.nodes[1]).toEqual({
      id: 'output',
      type: 'output',
      position: { x: 300, y: 0 },
      data: {
        label: 'Output',
        config: {},
        runtime: { provider: 'production', pool: 'output-pool', secrets: [] },
      },
    });
    expect(merged.edges[0]).toEqual({
      id: 'edge',
      source: 'start',
      target: 'output',
    });
  });

  test('preserves a named router source handle when saving', () => {
    const merged = mergeCanvas(workflow, toCanvasNodes(workflow, []), [
      {
        id: 'route',
        source: 'router',
        target: 'output',
        sourceHandle: 'approved',
      },
    ]);

    expect(merged.edges[0].sourceHandle).toBe('approved');
  });
});
