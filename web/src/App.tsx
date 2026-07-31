import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
  ReactFlow,
  addEdge,
  useEdgesState,
  useNodesState,
  type Connection,
  type Edge,
  type NodeTypes,
} from '@xyflow/react';
import { api } from './api';
import {
  mergeCanvas,
  toCanvasEdges,
  toCanvasNodes,
  type StudioNode,
} from './graph';
import type {
  NodeDescriptor,
  NodeKind,
  RuntimeEvidence,
  Workflow,
  WorkflowRun,
} from './types';
import { Inspector } from './components/Inspector';
import { NodeIcon } from './components/NodeIcon';
import { WorkflowCardNode } from './components/WorkflowCardNode';

const nodeTypes: NodeTypes = { workflow: WorkflowCardNode };

const groups: Array<{ label: string; kinds: NodeKind[] }> = [
  { label: 'Intelligence', kinds: ['llm', 'agent', 'memory'] },
  { label: 'Flow control', kinds: ['template', 'router', 'approval'] },
  { label: 'Actions', kinds: ['tool', 'http'] },
  { label: 'Boundaries', kinds: ['start', 'output'] },
];

export function App() {
  const [workflows, setWorkflows] = useState<Workflow[]>([]);
  const [workflow, setWorkflow] = useState<Workflow>();
  const [catalog, setCatalog] = useState<NodeDescriptor[]>([]);
  const [nodes, setNodes, onNodesChange] = useNodesState<StudioNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [inputSource, setInputSource] = useState('{\n  "name": "Ada"\n}');
  const [run, setRun] = useState<WorkflowRun>();
  const [evidence, setEvidence] = useState<RuntimeEvidence[]>([]);
  const [executionOpen, setExecutionOpen] = useState(true);
  const [saving, setSaving] = useState(false);
  const [running, setRunning] = useState(false);
  const [notice, setNotice] = useState('');
  const [error, setError] = useState('');
  const pollEpoch = useRef(0);

  useEffect(() => {
    void Promise.all([api.listWorkflows(), api.listNodeTypes()])
      .then(([items, descriptors]) => {
        setWorkflows(items);
        setCatalog(descriptors);
        if (items[0]) selectWorkflow(items[0]);
      })
      .catch(showError);
  }, []);

  const selectWorkflow = useCallback(
    (next: Workflow) => {
      setWorkflow(next);
      setNodes(toCanvasNodes(next, []));
      setEdges(toCanvasEdges(next.edges));
      setSelectedId(undefined);
      setRun(undefined);
      setEvidence([]);
      setError('');
    },
    [setEdges, setNodes],
  );

  const selectedNode = nodes.find((node) => node.id === selectedId);
  const selectedEvidence = evidence.find((item) => item.nodeId === selectedId);

  const updateNode = useCallback(
    (nodeId: string, data: StudioNode['data']) => {
      setNodes((items) =>
        items.map((item) => (item.id === nodeId ? { ...item, data } : item)),
      );
    },
    [setNodes],
  );

  const deleteNode = useCallback(
    (nodeId: string) => {
      setNodes((items) => items.filter((item) => item.id !== nodeId));
      setEdges((items) =>
        items.filter((item) => item.source !== nodeId && item.target !== nodeId),
      );
      setSelectedId(undefined);
    },
    [setEdges, setNodes],
  );

  const addNode = (descriptor: NodeDescriptor) => {
    if (
      ['start', 'output'].includes(descriptor.kind) &&
      nodes.some((node) => node.data.kind === descriptor.kind)
    ) {
      setNotice(`This workflow already has a ${descriptor.kind} node.`);
      return;
    }
    const id = `${descriptor.kind}-${crypto.randomUUID().slice(0, 8)}`;
    const next: StudioNode = {
      id,
      type: 'workflow',
      position: { x: 160 + (nodes.length % 3) * 280, y: 120 + nodes.length * 72 },
      data: {
        label: descriptor.label,
        kind: descriptor.kind,
        config: descriptor.defaultConfig,
        runtime: { secrets: [] },
      },
    };
    setNodes((items) => [...items, next]);
    setSelectedId(id);
    setNotice(`${descriptor.label} node added.`);
  };

  const connect = useCallback(
    (connection: Connection) => {
      if (!connection.source || !connection.target) return;
      setEdges((items) =>
        addEdge(
          {
            ...connection,
            id: `edge-${crypto.randomUUID().slice(0, 8)}`,
            type: 'smoothstep',
            style: { stroke: '#526078', strokeWidth: 1.5 },
          },
          items,
        ),
      );
    },
    [setEdges],
  );

  const saveWorkflow = useCallback(async (): Promise<Workflow> => {
    if (!workflow) throw new Error('No workflow selected');
    setSaving(true);
    setError('');
    try {
      const updated = await api.updateWorkflow(mergeCanvas(workflow, nodes, edges));
      setWorkflow(updated);
      setWorkflows((items) =>
        items.map((item) => (item.id === updated.id ? updated : item)),
      );
      setNotice(`Saved version ${updated.version}.`);
      return updated;
    } finally {
      setSaving(false);
    }
  }, [edges, nodes, workflow]);

  const startRun = async () => {
    setError('');
    setRunning(true);
    try {
      const input = JSON.parse(inputSource);
      const saved = await saveWorkflow();
      const next = await api.startRun(saved.id, input);
      setRun(next);
      setEvidence([]);
      setExecutionOpen(true);
      pollEpoch.current += 1;
      void pollRun(next.run_id, pollEpoch.current);
    } catch (reason) {
      showError(reason);
      setRunning(false);
    }
  };

  const pollRun = async (runId: string, epoch: number) => {
    try {
      const [next, nextEvidence] = await Promise.all([
        api.getRun(runId),
        api.listRuntimeEvidence(runId),
      ]);
      if (epoch !== pollEpoch.current) return;
      setRun(next);
      setEvidence(nextEvidence);
      setNodes((items) =>
        items.map((item) => {
          const execution = nextEvidence.find((entry) => entry.nodeId === item.id);
          return {
            ...item,
            data: {
              ...item.data,
              executionState: execution?.state,
              runtimeUnit: execution?.unitId,
            },
          };
        }),
      );
      if (next.status === 'running') {
        window.setTimeout(() => void pollRun(runId, epoch), 350);
      } else {
        setRunning(false);
        setNotice(`Run ${next.status}.`);
      }
    } catch (reason) {
      setRunning(false);
      showError(reason);
    }
  };

  const approve = async (nodeId: string) => {
    if (!run) return;
    try {
      await api.approve(run.run_id, nodeId, { approved: true, actor: 'studio' });
      setRunning(true);
      pollEpoch.current += 1;
      void pollRun(run.run_id, pollEpoch.current);
    } catch (reason) {
      showError(reason);
    }
  };

  function showError(reason: unknown) {
    setError(reason instanceof Error ? reason.message : String(reason));
  }

  const evidenceByNode = useMemo(
    () => new Map(evidence.map((item) => [item.nodeId, item])),
    [evidence],
  );
  const completedNodes = evidence.filter((item) => item.state === 'succeeded').length;

  return (
    <main className="app-shell">
      <nav className="rail" aria-label="Primary navigation">
        <div className="brand-mark" aria-label="A3S"><span>A</span><i /></div>
        <button className="rail-button active" aria-label="Workflow studio"><GridIcon /></button>
        <button className="rail-button" aria-label="Runtime providers"><RuntimeIcon /></button>
        <div className="rail-spacer" />
        <span className="system-online" title="Control plane online" />
        <button className="avatar" aria-label="Account">A3</button>
      </nav>

      <aside className="library" aria-label="Node library">
        <div className="library-brand">
          <span>A3S WORKFLOW</span>
          <strong>AI Native Studio</strong>
        </div>
        <label className="search-box">
          <SearchIcon />
          <input aria-label="Search nodes" placeholder="Search nodes" />
          <kbd>⌘K</kbd>
        </label>
        <div className="library-scroll">
          {groups.map((group) => (
            <section key={group.label} className="node-group">
              <h2>{group.label}</h2>
              {group.kinds.map((kind) => {
                const descriptor = catalog.find((item) => item.kind === kind);
                if (!descriptor) return null;
                return (
                  <button
                    type="button"
                    className="library-node"
                    key={kind}
                    onClick={() => addNode(descriptor)}
                    data-testid={`add-node-${kind}`}
                  >
                    <span className={`node-icon kind-${kind}`}><NodeIcon kind={kind} /></span>
                    <span><strong>{descriptor.label}</strong><small>{shortDescription(descriptor.description)}</small></span>
                    <i>+</i>
                  </button>
                );
              })}
            </section>
          ))}
        </div>
      </aside>

      <section className="workspace">
        <header className="topbar">
          <div className="workflow-switcher">
            <span className="eyebrow">Workflow</span>
            <select
              aria-label="Select workflow"
              value={workflow?.id ?? ''}
              onChange={(event) => {
                const next = workflows.find((item) => item.id === event.target.value);
                if (next) selectWorkflow(next);
              }}
            >
              {workflows.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
            </select>
            <span className="version-chip">v{workflow?.version ?? '—'}</span>
          </div>
          <div className="topbar-meta">
            <span><i className="status-light" />PostgreSQL durable</span>
            <span>{nodes.length} Runtime units</span>
          </div>
          <div className="topbar-actions">
            <button
              type="button"
              className="secondary-button"
              onClick={() => void saveWorkflow().catch(showError)}
              disabled={!workflow || saving}
              data-testid="save-workflow"
            >
              {saving ? 'Saving…' : 'Save'}
            </button>
            <button
              type="button"
              className="run-button"
              onClick={() => void startRun()}
              disabled={!workflow || running}
              data-testid="run-workflow"
            >
              <PlayIcon /> {running ? 'Running' : 'Run graph'}
            </button>
          </div>
        </header>

        <div className="canvas-wrap" data-testid="workflow-canvas">
          <ReactFlow<StudioNode, Edge>
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={connect}
            onNodeClick={(_, node) => setSelectedId(node.id)}
            onPaneClick={() => setSelectedId(undefined)}
            fitView
            fitViewOptions={{ padding: 0.2 }}
            minZoom={0.25}
            maxZoom={1.8}
            deleteKeyCode={['Backspace', 'Delete']}
            proOptions={{ hideAttribution: true }}
          >
            <Background variant={BackgroundVariant.Dots} gap={22} size={1.1} color="#343b51" />
            <Controls position="bottom-left" showInteractive={false} />
            <MiniMap
              position="bottom-right"
              pannable
              zoomable
              nodeColor={(node) => nodeColor((node as StudioNode).data.kind)}
              maskColor="rgba(12, 14, 24, .72)"
            />
          </ReactFlow>
          <div className="canvas-caption">
            <span>CONTROL PLANE</span>
            <strong>Every card crosses the A3S Runtime boundary</strong>
          </div>
        </div>

        <section className={`execution-dock${executionOpen ? ' open' : ''}`} aria-label="Execution console">
          <button className="dock-toggle" type="button" onClick={() => setExecutionOpen((value) => !value)}>
            <span><TerminalIcon /> Execution</span>
            <span className={`status-pill state-${run?.status ?? 'idle'}`}>{run?.status ?? 'idle'}</span>
            <span>{run ? `${completedNodes}/${nodes.length} units` : 'No active run'}</span>
            <i>{executionOpen ? '⌄' : '⌃'}</i>
          </button>
          {executionOpen && (
            <div className="dock-body">
              <div className="run-input-panel">
                <label>Run input · JSON</label>
                <textarea
                  aria-label="Run input JSON"
                  data-testid="run-input"
                  value={inputSource}
                  onChange={(event) => setInputSource(event.target.value)}
                  spellCheck={false}
                />
              </div>
              <div className="execution-track" data-testid="execution-track">
                <div className="track-header">
                  <span>Runtime units</span>
                  {run && <code>{run.run_id.slice(0, 8)}</code>}
                </div>
                <div className="track-items">
                  {nodes.map((node, index) => {
                    const item = evidenceByNode.get(node.id);
                    return (
                      <button
                        type="button"
                        key={node.id}
                        className="track-item"
                        onClick={() => setSelectedId(node.id)}
                      >
                        <span className={`track-index state-${item?.state ?? 'waiting'}`}>{index + 1}</span>
                        <span><strong>{node.data.label}</strong><small>{item ? `${item.providerId} · gen ${item.generation}` : 'waiting'}</small></span>
                      </button>
                    );
                  })}
                </div>
              </div>
              <div className="run-output" data-testid="run-output">
                <div className="track-header"><span>Output</span><span>{run?.error ? 'error' : 'JSON'}</span></div>
                <pre>{JSON.stringify(run?.error ?? run?.output ?? { status: 'Awaiting run' }, null, 2)}</pre>
                {run && Object.entries(run.hooks ?? {}).map(([nodeId, hook]) =>
                  hook.status === 'active' ? (
                    <button key={nodeId} className="approval-button" onClick={() => void approve(nodeId)}>
                      Approve · {hook.metadata?.subject ?? nodeId}
                    </button>
                  ) : null,
                )}
              </div>
            </div>
          )}
        </section>
      </section>

      <Inspector
        node={selectedNode}
        evidence={selectedEvidence}
        onChange={updateNode}
        onDelete={deleteNode}
      />

      {(notice || error) && (
        <button
          type="button"
          className={`toast${error ? ' error' : ''}`}
          onClick={() => { setNotice(''); setError(''); }}
        >
          {error || notice}
        </button>
      )}
    </main>
  );
}

function nodeColor(kind: NodeKind) {
  if (['llm', 'agent'].includes(kind)) return '#7a5cff';
  if (['tool', 'http'].includes(kind)) return '#21b7a8';
  if (kind === 'approval') return '#e2a93b';
  return '#2587f5';
}

function shortDescription(value: string) {
  return value.replace(/\.$/, '').split(' ').slice(0, 5).join(' ');
}

function GridIcon() {
  return <svg viewBox="0 0 24 24"><rect x="4" y="4" width="6" height="6" rx="1"/><rect x="14" y="4" width="6" height="6" rx="1"/><rect x="4" y="14" width="6" height="6" rx="1"/><rect x="14" y="14" width="6" height="6" rx="1"/></svg>;
}
function RuntimeIcon() {
  return <svg viewBox="0 0 24 24"><path d="M5 8l7-4 7 4-7 4-7-4zM5 12l7 4 7-4M5 16l7 4 7-4"/></svg>;
}
function SearchIcon() {
  return <svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="6"/><path d="M16 16l4 4"/></svg>;
}
function PlayIcon() {
  return <svg viewBox="0 0 24 24"><path d="M8 5l11 7-11 7V5z"/></svg>;
}
function TerminalIcon() {
  return <svg viewBox="0 0 24 24"><path d="M5 7l4 5-4 5M11 17h8"/></svg>;
}
