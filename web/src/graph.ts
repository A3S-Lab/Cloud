import type { Node, Edge } from '@xyflow/react';
import type { RuntimeEvidence, Workflow, WorkflowEdge, WorkflowNode } from './types';

export type CanvasEdgeType = 'bezier' | 'smoothstep' | 'straight';

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

export function toCanvasEdges(
  edges: WorkflowEdge[],
  edgeType: CanvasEdgeType = 'bezier',
): Edge[] {
  return edges.map((edge) => ({
    ...edge,
    type: edgeType,
    animated: false,
    className: 'workflow-edge',
  }));
}

export function mergeCanvas(
  workflow: Workflow,
  nodes: StudioNode[],
  edges: Edge[],
): Workflow {
  return {
    ...workflow,
    nodes: nodes.map((node) => {
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
