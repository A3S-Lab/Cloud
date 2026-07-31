import { describe, expect, test } from 'bun:test';
import { mergeCanvas, toCanvasEdges, toCanvasNodes } from './graph';
import type { Workflow } from './types';

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
});
