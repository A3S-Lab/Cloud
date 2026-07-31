import type {
  NodeDescriptor,
  RuntimeEvidence,
  Workflow,
  WorkflowRun,
} from './types';

const API_ROOT = '/api/v1';

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_ROOT}${path}`, {
    ...init,
    headers: {
      'content-type': 'application/json',
      ...init?.headers,
    },
  });
  if (!response.ok) {
    const payload = await response.text();
    throw new Error(payload || `Request failed with ${response.status}`);
  }
  if (response.status === 204) {
    return undefined as T;
  }
  return response.json() as Promise<T>;
}

export const api = {
  listWorkflows: () => request<Workflow[]>('/workflows'),
  listNodeTypes: () => request<NodeDescriptor[]>('/node-types'),
  updateWorkflow: (workflow: Workflow) =>
    request<Workflow>(`/workflows/${workflow.id}`, {
      method: 'PUT',
      body: JSON.stringify({
        version: workflow.version,
        name: workflow.name,
        description: workflow.description,
        nodes: workflow.nodes,
        edges: workflow.edges,
      }),
    }),
  startRun: (workflowId: string, input: unknown) =>
    request<WorkflowRun>(`/workflows/${workflowId}/runs`, {
      method: 'POST',
      body: JSON.stringify({ input }),
    }),
  getRun: (runId: string) => request<WorkflowRun>(`/runs/${runId}`),
  listRuntimeEvidence: (runId: string) =>
    request<RuntimeEvidence[]>(`/runs/${runId}/node-executions`),
  approve: (runId: string, nodeId: string, payload: unknown) =>
    request<WorkflowRun>(`/runs/${runId}/approvals/${nodeId}`, {
      method: 'POST',
      body: JSON.stringify({ payload }),
    }),
};
