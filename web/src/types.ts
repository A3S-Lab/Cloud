export type NodeKind =
  | 'start'
  | 'template'
  | 'llm'
  | 'agent'
  | 'tool'
  | 'router'
  | 'memory'
  | 'http'
  | 'approval'
  | 'output';

export type RuntimePolicy = {
  provider?: string;
  pool?: string;
  cpuMillis?: number;
  memoryBytes?: number;
  pids?: number;
  timeoutMs?: number;
  isolation?: 'process' | 'container' | 'sandbox' | 'confidential';
  network?: 'none' | 'outbound';
  secrets: Array<{
    name: string;
    reference: string;
    target: Record<string, unknown>;
  }>;
};

export type WorkflowNode = {
  id: string;
  type: NodeKind;
  position: { x: number; y: number };
  data: {
    label: string;
    config: unknown;
    runtime: RuntimePolicy;
  };
};

export type WorkflowEdge = {
  id: string;
  source: string;
  target: string;
  sourceHandle?: string;
};

export type Workflow = {
  id: string;
  name: string;
  description: string;
  version: number;
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
  createdAt: string;
  updatedAt: string;
};

export type NodeDescriptor = {
  kind: NodeKind;
  label: string;
  description: string;
  defaultConfig: unknown;
};

export type RunStatus = 'running' | 'completed' | 'failed' | 'cancelled';

export type WorkflowRun = {
  run_id: string;
  status: RunStatus;
  output: unknown;
  error: string | null;
  steps: Record<
    string,
    {
      step_id: string;
      status: string;
      attempt: number;
      output?: { output?: unknown; metadata?: Record<string, unknown> };
      error?: string | null;
    }
  >;
  hooks: Record<
    string,
    {
      hook_id: string;
      status: string;
      metadata: {
        subject?: string;
        data?: Record<string, unknown>;
      };
    }
  >;
};

export type RuntimeEvidence = {
  executionId: string;
  runId: string;
  stepId: string;
  attempt: number;
  nodeId: string;
  providerId: string;
  runtimePool: string | null;
  unitId: string | null;
  generation: number | null;
  specDigest: string | null;
  state: string;
  observation?: {
    provider_build?: string;
    provider_resource_id?: string;
    usage?: { wall_time_ms?: number };
    outputs?: Array<{ artifact: { digest: string } }>;
  } | null;
};
