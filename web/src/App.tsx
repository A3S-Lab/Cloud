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
const TERMINAL_EVIDENCE_POLL_LIMIT = 4;

const groups: Array<{ label: string; description: string; kinds: NodeKind[] }> = [
  {
    label: 'AI & agents',
    description: 'Reason, plan, and retain context',
    kinds: ['llm', 'agent', 'memory'],
  },
  {
    label: 'Logic',
    description: 'Shape and control the workflow',
    kinds: ['template', 'router', 'approval'],
  },
  {
    label: 'Integrations',
    description: 'Call tools and external services',
    kinds: ['tool', 'http'],
  },
  {
    label: 'Input & output',
    description: 'Define workflow boundaries',
    kinds: ['start', 'output'],
  },
];

type RunTab = 'RESULT' | 'DETAIL' | 'TRACING';

export function App() {
  const [workflows, setWorkflows] = useState<Workflow[]>([]);
  const [workflow, setWorkflow] = useState<Workflow>();
  const [catalog, setCatalog] = useState<NodeDescriptor[]>([]);
  const [nodes, setNodes, onNodesChange] = useNodesState<StudioNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [libraryOpen, setLibraryOpen] = useState(false);
  const [catalogQuery, setCatalogQuery] = useState('');
  const [minimapOpen, setMinimapOpen] = useState(true);
  const [inputSource, setInputSource] = useState('{\n  "name": "Ada"\n}');
  const [run, setRun] = useState<WorkflowRun>();
  const [evidence, setEvidence] = useState<RuntimeEvidence[]>([]);
  const [runPanelOpen, setRunPanelOpen] = useState(false);
  const [runTab, setRunTab] = useState<RunTab>('RESULT');
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

  useEffect(() => {
    const handleShortcut = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        setSelectedId(undefined);
        setRunPanelOpen(false);
        setLibraryOpen(true);
      }
      if (event.key === 'Escape') {
        setLibraryOpen(false);
        setRunPanelOpen(false);
        setSelectedId(undefined);
      }
    };
    window.addEventListener('keydown', handleShortcut);
    return () => window.removeEventListener('keydown', handleShortcut);
  }, []);

  const selectWorkflow = useCallback(
    (next: Workflow) => {
      setWorkflow(next);
      setNodes(toCanvasNodes(next, []));
      setEdges(toCanvasEdges(next.edges));
      setSelectedId(undefined);
      setRun(undefined);
      setEvidence([]);
      setRunPanelOpen(false);
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
      position: { x: 160 + (nodes.length % 3) * 300, y: 120 + nodes.length * 76 },
      data: {
        label: descriptor.label,
        kind: descriptor.kind,
        config: descriptor.defaultConfig,
        runtime: { secrets: [] },
      },
    };
    setNodes((items) => [...items, next]);
    setSelectedId(id);
    setLibraryOpen(false);
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
            style: { stroke: '#98a2b3', strokeWidth: 1.5 },
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
    setRunPanelOpen(true);
    setRunTab('RESULT');
    try {
      const input = JSON.parse(inputSource);
      const saved = await saveWorkflow();
      const next = await api.startRun(saved.id, input);
      setRun(next);
      setEvidence([]);
      pollEpoch.current += 1;
      void pollRun(next.run_id, pollEpoch.current, nodes.length);
    } catch (reason) {
      showError(reason);
      setRunning(false);
    }
  };

  const pollRun = async (
    runId: string,
    epoch: number,
    expectedNodeCount: number,
    terminalEvidencePoll = 0,
  ) => {
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
      const completedRuntimeUnits = countSucceededRuntimeUnits(nextEvidence);
      const evidenceStillSettling =
        next.status === 'completed' &&
        completedRuntimeUnits < expectedNodeCount &&
        terminalEvidencePoll < TERMINAL_EVIDENCE_POLL_LIMIT;
      if (next.status === 'running' || evidenceStillSettling) {
        const nextTerminalPoll = next.status === 'running' ? 0 : terminalEvidencePoll + 1;
        window.setTimeout(
          () => void pollRun(runId, epoch, expectedNodeCount, nextTerminalPoll),
          350,
        );
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
      void pollRun(run.run_id, pollEpoch.current, nodes.length);
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
  const completedNodes = useMemo(() => countSucceededRuntimeUnits(evidence), [evidence]);

  return (
    <main className="app-shell">
      <aside className="product-rail" aria-label="Primary navigation">
        <div className="brand-mark" aria-label="A3S Workflow">A</div>
        <nav>
          <button
            type="button"
            className={!runPanelOpen ? 'rail-button active' : 'rail-button'}
            aria-label="Workflow editor"
            title="Workflow editor"
            onClick={() => setRunPanelOpen(false)}
          >
            <WorkflowIcon />
          </button>
          <button
            type="button"
            className={runPanelOpen ? 'rail-button active' : 'rail-button'}
            aria-label="Runtime runs"
            title="Runtime runs"
            onClick={() => {
              setRunPanelOpen(true);
              setLibraryOpen(false);
              setSelectedId(undefined);
            }}
          >
            <PulseIcon />
          </button>
        </nav>
        <div className="rail-runtime" title="A3S Runtime connected"><span /></div>
      </aside>

      <section className="studio-shell">
        <header className="studio-header">
          <div className="workflow-identity">
            <span className="workflow-badge"><WorkflowIcon /></span>
            <div className="workflow-picker">
              <span>Studio / Workflow</span>
              <div>
                <select
                  aria-label="Select workflow"
                  value={workflow?.id ?? ''}
                  onChange={(event) => {
                    const next = workflows.find((item) => item.id === event.target.value);
                    if (next) selectWorkflow(next);
                  }}
                >
                  {workflows.map((item) => (
                    <option key={item.id} value={item.id}>{item.name}</option>
                  ))}
                </select>
                <ChevronDownIcon />
              </div>
            </div>
            <span className="version-chip">v{workflow?.version ?? '—'}</span>
          </div>

          <div className="header-status">
            <span className="durable-status"><i /> PostgreSQL durable</span>
            <span className="runtime-count">{nodes.length} Runtime units</span>
          </div>

          <div className="header-actions">
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
              onClick={() => {
                setRunPanelOpen(true);
                setLibraryOpen(false);
                setSelectedId(undefined);
              }}
              disabled={!workflow}
              data-testid="open-run-panel"
            >
              <PlayIcon /> Test Run
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
            onNodeClick={(_, node) => {
              setSelectedId(node.id);
              setRunPanelOpen(false);
              setLibraryOpen(false);
            }}
            onPaneClick={() => {
              setSelectedId(undefined);
              setLibraryOpen(false);
            }}
            fitView
            fitViewOptions={{ padding: 0.24 }}
            minZoom={0.25}
            maxZoom={1.8}
            deleteKeyCode={['Backspace', 'Delete']}
            proOptions={{ hideAttribution: true }}
          >
            <Background variant={BackgroundVariant.Dots} gap={20} size={1} color="#d0d5dd" />
            <Controls position="bottom-left" showInteractive={false} />
            {minimapOpen && (
              <MiniMap
                position="bottom-right"
                pannable
                zoomable
                nodeColor={(node) => nodeColor((node as StudioNode).data.kind)}
                maskColor="rgba(249, 250, 251, .72)"
              />
            )}
          </ReactFlow>

          <div className="canvas-context">
            <span className="context-icon"><RuntimeIcon /></span>
            <div><strong>A3S Runtime graph</strong><span>Every node is independently placed and executed</span></div>
          </div>

          <div className="canvas-operators" aria-label="Canvas tools">
            <button
              type="button"
              className={libraryOpen ? 'operator-button primary active' : 'operator-button primary'}
              onClick={() => {
                setLibraryOpen((value) => !value);
                setRunPanelOpen(false);
                setSelectedId(undefined);
              }}
              data-testid="open-node-library"
            >
              <PlusIcon /> <span>Add node</span>
            </button>
            <button
              type="button"
              className={minimapOpen ? 'operator-button active' : 'operator-button'}
              aria-pressed={minimapOpen}
              aria-label="Toggle minimap"
              title="Toggle minimap"
              onClick={() => setMinimapOpen((value) => !value)}
            >
              <MapIcon />
            </button>
          </div>

          {libraryOpen && (
            <NodeLibrary
              catalog={catalog}
              query={catalogQuery}
              onQueryChange={setCatalogQuery}
              onAdd={addNode}
              onClose={() => setLibraryOpen(false)}
            />
          )}

          {selectedNode && !runPanelOpen && (
            <Inspector
              node={selectedNode}
              evidence={selectedEvidence}
              onChange={updateNode}
              onDelete={deleteNode}
              onClose={() => setSelectedId(undefined)}
            />
          )}

          {runPanelOpen && (
            <RunPanel
              nodes={nodes}
              run={run}
              evidenceByNode={evidenceByNode}
              completedNodes={completedNodes}
              inputSource={inputSource}
              running={running}
              activeTab={runTab}
              onTabChange={setRunTab}
              onInputChange={setInputSource}
              onStart={() => void startRun()}
              onApprove={(nodeId) => void approve(nodeId)}
              onInspect={(nodeId) => {
                setSelectedId(nodeId);
                setRunPanelOpen(false);
              }}
              onClose={() => setRunPanelOpen(false)}
            />
          )}
        </div>
      </section>

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

type NodeLibraryProps = {
  catalog: NodeDescriptor[];
  query: string;
  onQueryChange: (value: string) => void;
  onAdd: (descriptor: NodeDescriptor) => void;
  onClose: () => void;
};

function NodeLibrary({ catalog, query, onQueryChange, onAdd, onClose }: NodeLibraryProps) {
  const normalized = query.trim().toLowerCase();
  const filtered = catalog.filter((item) =>
    `${item.label} ${item.description} ${item.kind}`.toLowerCase().includes(normalized),
  );

  return (
    <aside className="node-library" aria-label="Node library">
      <header className="panel-header">
        <div><span>BUILDING BLOCKS</span><h2>Add node</h2></div>
        <button type="button" className="icon-button" aria-label="Close node library" onClick={onClose}>
          <CloseIcon />
        </button>
      </header>
      <label className="catalog-search">
        <SearchIcon />
        <input
          autoFocus
          aria-label="Search nodes"
          placeholder="Search nodes"
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
        />
        <kbd>⌘/Ctrl K</kbd>
      </label>
      <div className="catalog-scroll">
        {groups.map((group) => {
          const descriptors = group.kinds
            .map((kind) => filtered.find((item) => item.kind === kind))
            .filter((item): item is NodeDescriptor => Boolean(item));
          if (!descriptors.length) return null;
          return (
            <section className="node-group" key={group.label}>
              <div className="group-heading"><strong>{group.label}</strong><span>{group.description}</span></div>
              <div className="node-grid">
                {descriptors.map((descriptor) => (
                  <button
                    type="button"
                    className="library-node"
                    key={descriptor.kind}
                    onClick={() => onAdd(descriptor)}
                    data-testid={`add-node-${descriptor.kind}`}
                  >
                    <span className={`node-icon kind-${descriptor.kind}`}><NodeIcon kind={descriptor.kind} /></span>
                    <span><strong>{descriptor.label}</strong><small>{descriptor.description}</small></span>
                    <PlusIcon />
                  </button>
                ))}
              </div>
            </section>
          );
        })}
        {!filtered.length && (
          <div className="catalog-empty"><SearchIcon /><strong>No matching nodes</strong><span>Try another name or capability.</span></div>
        )}
      </div>
      <footer className="catalog-footer"><RuntimeIcon /> All nodes execute through the A3S Runtime contract</footer>
    </aside>
  );
}

type RunPanelProps = {
  nodes: StudioNode[];
  run?: WorkflowRun;
  evidenceByNode: Map<string, RuntimeEvidence>;
  completedNodes: number;
  inputSource: string;
  running: boolean;
  activeTab: RunTab;
  onTabChange: (tab: RunTab) => void;
  onInputChange: (value: string) => void;
  onStart: () => void;
  onApprove: (nodeId: string) => void;
  onInspect: (nodeId: string) => void;
  onClose: () => void;
};

function RunPanel({
  nodes,
  run,
  evidenceByNode,
  completedNodes,
  inputSource,
  running,
  activeTab,
  onTabChange,
  onInputChange,
  onStart,
  onApprove,
  onInspect,
  onClose,
}: RunPanelProps) {
  const tabs: RunTab[] = ['RESULT', 'DETAIL', 'TRACING'];
  const output = run?.error ?? run?.output ?? { status: 'Ready for a test run' };

  return (
    <aside className="run-panel" aria-label="Test run" data-testid="execution-console">
      <header className="panel-header run-panel-header">
        <div><span>DEBUG WORKFLOW</span><h2>Test Run</h2></div>
        <div className="run-panel-heading-meta">
          <span className={`status-pill state-${run?.status ?? 'idle'}`}>{run?.status ?? 'idle'}</span>
          <span>{run ? `${completedNodes}/${nodes.length} units` : `${nodes.length} Runtime units`}</span>
          <button type="button" className="icon-button" aria-label="Close test run" onClick={onClose}><CloseIcon /></button>
        </div>
      </header>

      <nav className="panel-tabs" aria-label="Run views">
        {tabs.map((tab) => (
          <button
            type="button"
            className={activeTab === tab ? 'active' : ''}
            key={tab}
            onClick={() => onTabChange(tab)}
          >
            {tab}
            {tab === 'TRACING' && evidenceByNode.size > 0 && <span>{evidenceByNode.size}</span>}
          </button>
        ))}
      </nav>

      <div className="run-panel-body">
        {activeTab === 'RESULT' && (
          <div className="result-view">
            <label className="run-input-field">
              <span>Workflow inputs <em>JSON</em></span>
              <textarea
                aria-label="Run input JSON"
                data-testid="run-input"
                value={inputSource}
                onChange={(event) => onInputChange(event.target.value)}
                spellCheck={false}
              />
            </label>
            <section className="output-card" data-testid="run-output">
              <header><span>Final output</span><span>{run?.error ? 'ERROR' : 'JSON'}</span></header>
              <pre>{JSON.stringify(output, null, 2)}</pre>
            </section>
            {run && Object.entries(run.hooks ?? {}).map(([nodeId, hook]) =>
              hook.status === 'active' ? (
                <button key={nodeId} className="approval-button" onClick={() => onApprove(nodeId)}>
                  <ApprovalIcon /> Approve · {hook.metadata?.subject ?? nodeId}
                </button>
              ) : null,
            )}
          </div>
        )}

        {activeTab === 'DETAIL' && (
          <div className="detail-view">
            <section className="run-summary">
              <div><span>Status</span><strong className={`state-text-${run?.status ?? 'idle'}`}>{run?.status ?? 'Not started'}</strong></div>
              <div><span>Runtime units</span><strong>{completedNodes} / {nodes.length}</strong></div>
              <div><span>Durability</span><strong>PostgreSQL</strong></div>
              <div><span>Execution boundary</span><strong>A3S Runtime</strong></div>
            </section>
            <section className="detail-card">
              <span>Run ID</span>
              <code>{run?.run_id ?? 'Created when the test starts'}</code>
            </section>
            <section className="detail-card">
              <span>Input</span>
              <pre>{inputSource}</pre>
            </section>
          </div>
        )}

        {activeTab === 'TRACING' && (
          <div className="trace-list" data-testid="execution-track">
            {nodes.map((node, index) => {
              const item = evidenceByNode.get(node.id);
              return (
                <button type="button" className="trace-card" key={node.id} onClick={() => onInspect(node.id)}>
                  <span className={`trace-status state-${item?.state ?? 'waiting'}`}>
                    {item?.state === 'succeeded' ? <CheckIcon /> : index + 1}
                  </span>
                  <span className={`node-icon kind-${node.data.kind}`}><NodeIcon kind={node.data.kind} /></span>
                  <span className="trace-copy">
                    <strong>{node.data.label}</strong>
                    <small>{item ? `${item.providerId} · generation ${item.generation}` : 'Waiting for execution'}</small>
                  </span>
                  <span className={`trace-state state-text-${item?.state ?? 'waiting'}`}>{item?.state ?? 'waiting'}</span>
                  <ChevronRightIcon />
                </button>
              );
            })}
          </div>
        )}
      </div>

      <footer className="run-panel-footer">
        <span><RuntimeIcon /> Executed outside the control plane</span>
        <button
          type="button"
          className="run-button"
          onClick={onStart}
          disabled={running}
          data-testid="run-workflow"
        >
          <PlayIcon /> {running ? 'Running…' : 'Start run'}
        </button>
      </footer>
    </aside>
  );
}

function nodeColor(kind: NodeKind) {
  if (['llm', 'agent'].includes(kind)) return '#7f56d9';
  if (['tool', 'http', 'memory'].includes(kind)) return '#12b76a';
  if (kind === 'approval') return '#f79009';
  if (kind === 'router') return '#9e77ed';
  return '#2970ff';
}

function countSucceededRuntimeUnits(items: RuntimeEvidence[]) {
  return new Set(
    items.filter((item) => item.state === 'succeeded').map((item) => item.nodeId),
  ).size;
}

function SvgIcon({ children }: { children: React.ReactNode }) {
  return <svg viewBox="0 0 24 24" aria-hidden="true">{children}</svg>;
}
function PlayIcon() { return <SvgIcon><path d="M8 5l11 7-11 7V5z" /></SvgIcon>; }
function PlusIcon() { return <SvgIcon><path d="M12 5v14M5 12h14" /></SvgIcon>; }
function CloseIcon() { return <SvgIcon><path d="M6 6l12 12M18 6L6 18" /></SvgIcon>; }
function SearchIcon() { return <SvgIcon><circle cx="11" cy="11" r="6.5" /><path d="M16 16l4 4" /></SvgIcon>; }
function MapIcon() { return <SvgIcon><path d="M4 6l5-2 6 2 5-2v14l-5 2-6-2-5 2V6zM9 4v14M15 6v14" /></SvgIcon>; }
function ChevronDownIcon() { return <SvgIcon><path d="M7 9l5 5 5-5" /></SvgIcon>; }
function ChevronRightIcon() { return <SvgIcon><path d="M9 6l6 6-6 6" /></SvgIcon>; }
function CheckIcon() { return <SvgIcon><path d="M5 12l4 4L19 6" /></SvgIcon>; }
function ApprovalIcon() { return <SvgIcon><path d="M12 3l8 4v5c0 5-3.4 8-8 9-4.6-1-8-4-8-9V7l8-4z" /><path d="M8.5 12l2.2 2.2 4.8-5" /></SvgIcon>; }
function WorkflowIcon() { return <SvgIcon><rect x="4" y="4" width="6" height="6" rx="1.5" /><rect x="14" y="14" width="6" height="6" rx="1.5" /><path d="M10 7h3a4 4 0 014 4v3" /></SvgIcon>; }
function PulseIcon() { return <SvgIcon><path d="M3 12h4l2-6 4 12 2-6h6" /></SvgIcon>; }
function RuntimeIcon() { return <SvgIcon><path d="M12 3l8 4.5v9L12 21l-8-4.5v-9L12 3z" /><path d="M4 7.5l8 4.5 8-4.5M12 12v9" /></SvgIcon>; }
