import { useEffect, useState } from 'react';
import type { StudioNode } from '../graph';
import type { RuntimeEvidence } from '../types';
import { NodeIcon } from './NodeIcon';

type InspectorTab = 'CONFIG' | 'RUNTIME' | 'EVIDENCE';

type Props = {
  node?: StudioNode;
  evidence?: RuntimeEvidence;
  onChange: (nodeId: string, data: StudioNode['data']) => void;
  onDelete: (nodeId: string) => void;
  onClose?: () => void;
};

export function Inspector({ node, evidence, onChange, onDelete, onClose }: Props) {
  const [configSource, setConfigSource] = useState('{}');
  const [configError, setConfigError] = useState('');
  const [activeTab, setActiveTab] = useState<InspectorTab>('CONFIG');

  useEffect(() => {
    setConfigSource(JSON.stringify(node?.data.config ?? {}, null, 2));
    setConfigError('');
    setActiveTab('CONFIG');
  }, [node?.id, node?.data.config]);

  if (!node) {
    return (
      <aside className="node-panel empty-node-panel" aria-label="Node inspector">
        <div className="empty-state-icon"><CursorIcon /></div>
        <h2>No node selected</h2>
        <p>Select a node on the canvas to configure it.</p>
      </aside>
    );
  }

  const patch = (next: Partial<StudioNode['data']>) =>
    onChange(node.id, { ...node.data, ...next });
  const patchRuntime = (next: Partial<StudioNode['data']['runtime']>) =>
    patch({ runtime: { ...node.data.runtime, ...next } });
  const parseNumber = (value: string) => (value.trim() ? Number(value) : undefined);
  const applyConfig = () => {
    try {
      patch({ config: JSON.parse(configSource) });
      setConfigError('');
    } catch (error) {
      setConfigError(error instanceof Error ? error.message : 'Invalid JSON');
    }
  };

  return (
    <aside className="node-panel" aria-label="Node inspector" data-testid="node-inspector">
      <header className="panel-header node-panel-header">
        <div className="inspector-title">
          <span className={`node-icon kind-${node.data.kind}`}><NodeIcon kind={node.data.kind} /></span>
          <div><span>{node.data.kind.toUpperCase()} NODE</span><h2>{node.data.label}</h2></div>
        </div>
        <div className="panel-header-actions">
          {evidence && <span className={`status-pill state-${evidence.state}`}>{evidence.state}</span>}
          {onClose && (
            <button type="button" className="icon-button" aria-label="Close node inspector" onClick={onClose}>
              <CloseIcon />
            </button>
          )}
        </div>
      </header>

      <nav className="panel-tabs" aria-label="Node settings">
        {(['CONFIG', 'RUNTIME', 'EVIDENCE'] as InspectorTab[]).map((tab) => (
          <button
            type="button"
            className={activeTab === tab ? 'active' : ''}
            key={tab}
            onClick={() => setActiveTab(tab)}
          >
            {tab}
            {tab === 'EVIDENCE' && evidence && <span>1</span>}
          </button>
        ))}
      </nav>

      <div className="node-panel-body">
        {activeTab === 'CONFIG' && (
          <>
            <section className="inspector-section">
              <div className="section-heading"><span>GENERAL</span><small>Node identity</small></div>
              <label>
                Display name
                <input
                  aria-label="Display name"
                  value={node.data.label}
                  onChange={(event) => patch({ label: event.target.value })}
                />
              </label>
              <div className="readonly-field"><span>Node ID</span><code>{node.id}</code></div>
            </section>

            <section className="inspector-section config-section">
              <div className="section-heading"><span>NODE CONFIGURATION</span><small>Typed JSON</small></div>
              <textarea
                aria-label="Node configuration JSON"
                value={configSource}
                onChange={(event) => setConfigSource(event.target.value)}
                spellCheck={false}
              />
              {configError && <p className="field-error">{configError}</p>}
              <button className="secondary-button full" type="button" onClick={applyConfig}>
                Apply JSON
              </button>
            </section>
          </>
        )}

        {activeTab === 'RUNTIME' && (
          <>
            <div className="runtime-callout">
              <RuntimeIcon />
              <div><strong>Runs through A3S Runtime</strong><span>Placement is independent from the workflow control plane.</span></div>
            </div>

            <section className="inspector-section">
              <div className="section-heading"><span>PLACEMENT</span><small>Provider & pool</small></div>
              <div className="field-grid">
                <label>
                  Provider
                  <input
                    aria-label="Runtime provider"
                    placeholder="default"
                    value={node.data.runtime.provider ?? ''}
                    onChange={(event) => patchRuntime({ provider: event.target.value || undefined })}
                  />
                </label>
                <label>
                  Pool
                  <input
                    aria-label="Runtime pool"
                    placeholder="cpu / gpu"
                    value={node.data.runtime.pool ?? ''}
                    onChange={(event) => patchRuntime({ pool: event.target.value || undefined })}
                  />
                </label>
              </div>
              <div className="field-grid">
                <label>
                  Isolation
                  <select
                    aria-label="Runtime isolation"
                    value={node.data.runtime.isolation ?? 'process'}
                    onChange={(event) => patchRuntime({
                      isolation: event.target.value as StudioNode['data']['runtime']['isolation'],
                    })}
                  >
                    <option value="process">Process</option>
                    <option value="container">Container</option>
                    <option value="sandbox">Sandbox</option>
                    <option value="confidential">Confidential</option>
                  </select>
                </label>
                <label>
                  Network
                  <select
                    aria-label="Runtime network"
                    value={node.data.runtime.network ?? (['llm', 'agent', 'tool', 'memory', 'http'].includes(node.data.kind) ? 'outbound' : 'none')}
                    onChange={(event) => patchRuntime({ network: event.target.value as 'none' | 'outbound' })}
                  >
                    <option value="none">None</option>
                    <option value="outbound">Outbound</option>
                  </select>
                </label>
              </div>
            </section>

            <section className="inspector-section">
              <div className="section-heading"><span>RESOURCE ENVELOPE</span><small>Per execution</small></div>
              <div className="resource-grid">
                <label>
                  CPU · millicores
                  <input
                    type="number"
                    min="1"
                    value={node.data.runtime.cpuMillis ?? ''}
                    placeholder="500"
                    onChange={(event) => patchRuntime({ cpuMillis: parseNumber(event.target.value) })}
                  />
                </label>
                <label>
                  Memory · MiB
                  <input
                    type="number"
                    min="1"
                    value={node.data.runtime.memoryBytes ? Math.round(node.data.runtime.memoryBytes / 1024 / 1024) : ''}
                    placeholder="256"
                    onChange={(event) => patchRuntime({
                      memoryBytes: event.target.value ? Number(event.target.value) * 1024 * 1024 : undefined,
                    })}
                  />
                </label>
                <label>
                  Timeout · ms
                  <input
                    type="number"
                    min="1"
                    value={node.data.runtime.timeoutMs ?? ''}
                    placeholder="120000"
                    onChange={(event) => patchRuntime({ timeoutMs: parseNumber(event.target.value) })}
                  />
                </label>
              </div>
            </section>
          </>
        )}

        {activeTab === 'EVIDENCE' && (
          evidence ? (
            <section className="runtime-evidence" data-testid="runtime-evidence">
              <div className="evidence-heading">
                <span className={`status-pill state-${evidence.state}`}>{evidence.state}</span>
                <strong>Verified Runtime execution</strong>
              </div>
              <dl>
                <div><dt>Provider</dt><dd>{evidence.providerId}</dd></div>
                <div><dt>Pool</dt><dd>{evidence.runtimePool ?? 'default'}</dd></div>
                <div><dt>Generation</dt><dd>{evidence.generation ?? '—'}</dd></div>
                <div><dt>Unit</dt><dd title={evidence.unitId ?? ''}>{compact(evidence.unitId)}</dd></div>
                <div><dt>Spec digest</dt><dd title={evidence.specDigest ?? ''}>{compact(evidence.specDigest)}</dd></div>
              </dl>
            </section>
          ) : (
            <div className="evidence-empty">
              <RuntimeIcon />
              <h3>No Runtime evidence yet</h3>
              <p>Run the workflow to inspect provider placement, unit generation, and content digests.</p>
            </div>
          )
        )}
      </div>

      {!['start', 'output'].includes(node.data.kind) && (
        <footer className="node-panel-footer">
          <button className="danger-button" type="button" onClick={() => onDelete(node.id)}>
            <TrashIcon /> Delete node
          </button>
        </footer>
      )}
    </aside>
  );
}

function compact(value?: string | null) {
  if (!value) return '—';
  return value.length > 28 ? `${value.slice(0, 15)}…${value.slice(-9)}` : value;
}

function Icon({ children }: { children: React.ReactNode }) {
  return <svg viewBox="0 0 24 24" aria-hidden="true">{children}</svg>;
}
function CloseIcon() { return <Icon><path d="M6 6l12 12M18 6L6 18" /></Icon>; }
function CursorIcon() { return <Icon><path d="M5 3l13 9-7 2-3 7L5 3z" /></Icon>; }
function RuntimeIcon() { return <Icon><path d="M12 3l8 4.5v9L12 21l-8-4.5v-9L12 3z" /><path d="M4 7.5l8 4.5 8-4.5M12 12v9" /></Icon>; }
function TrashIcon() { return <Icon><path d="M5 7h14M9 7V4h6v3M7 7l1 13h8l1-13M10 11v5M14 11v5" /></Icon>; }
