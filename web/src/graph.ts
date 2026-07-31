import type { Node, Edge } from '@xyflow/react';
import type { RuntimeEvidence, Workflow, WorkflowEdge, WorkflowNode } from './types';

export type StudioNodeData = WorkflowNode['data'] & {
  kind: WorkflowNode['type'];
  executionState?: string;
  runtimeUnit?: string | null;
  [key: string]: unknown;
};

export type StudioNode = Node<StudioNodeData, 'workflow'>;

export function toCanvasNodes(
  workflow: Workflow,
  evidence: RuntimeEvidence[],
): StudioNode[] {
  const byNode = new Map(evidence.map((item) => [item.nodeId, item]));
  return workflow.nodes.map((node) => {
    const execution = byNode.get(node.id);
    return {
      id: node.id,
      type: 'workflow',
      position: node.position,
      data: {
        ...node.data,
        kind: node.type,
        executionState: execution?.state,
        runtimeUnit: execution?.unitId,
      },
    };
  });
}

export function toCanvasEdges(edges: WorkflowEdge[]): Edge[] {
  return edges.map((edge) => ({
    ...edge,
    type: 'smoothstep',
    animated: false,
    style: { stroke: '#526078', strokeWidth: 1.5 },
  }));
}

export function mergeCanvas(
  workflow: Workflow,
  nodes: StudioNode[],
  edges: Edge[],
): Workflow {
  const sourceById = new Map(workflow.nodes.map((node) => [node.id, node]));
  return {
    ...workflow,
    nodes: nodes.map((node) => {
      const source = sourceById.get(node.id);
      if (!source) {
        throw new Error(`Canvas contains unknown node ${node.id}`);
      }
      return {
        id: node.id,
        type: node.data.kind,
        position: node.position,
        data: {
          label: node.data.label,
          config: node.data.config,
          runtime: node.data.runtime,
        },
      };
    }),
    edges: edges.map((edge) => ({
      id: edge.id,
      source: edge.source,
      target: edge.target,
      ...(edge.sourceHandle ? { sourceHandle: edge.sourceHandle } : {}),
    })),
  };
}
