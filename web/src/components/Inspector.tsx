import { useEffect, useState } from 'react';
import type { StudioNode } from '../graph';
import type { RuntimeEvidence } from '../types';
import { NodeIcon } from './NodeIcon';

type Props = {
  node?: StudioNode;
  evidence?: RuntimeEvidence;
  onChange: (nodeId: string, data: StudioNode['data']) => void;
  onDelete: (nodeId: string) => void;
};

export function Inspector({ node, evidence, onChange, onDelete }: Props) {
  const [configSource, setConfigSource] = useState('{}');
  const [configError, setConfigError] = useState('');

  useEffect(() => {
    setConfigSource(JSON.stringify(node?.data.config ?? {}, null, 2));
    setConfigError('');
  }, [node?.id, node?.data.config]);

  if (!node) {
    return (
      <aside className="inspector empty-inspector" aria-label="Node inspector">
        <h2>No node selected</h2>
        <p>Select a Runtime node to edit its policy, resources, secrets, and typed configuration.</p>
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
    <aside className="inspector" aria-label="Node inspector" data-testid="node-inspector">
      <div className="inspector-title">
        <span className={`node-icon kind-${node.data.kind}`}>
          <NodeIcon kind={node.data.kind} />
        </span>
        <div>
          <span>{node.data.kind}</span>
          <h2>{node.data.label}</h2>
        </div>
      </div>

      <section className="inspector-section">
        <div className="section-heading"><span>01</span>Identity</div>
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

      <section className="inspector-section">
        <div className="section-heading"><span>02</span>Runtime placement</div>
        <div className="field-grid">
          <label>
            Provider
            <input
              aria-label="Runtime provider"
              placeholder="default"
              value={node.data.runtime.provider ?? ''}
              onChange={(event) =>
                patchRuntime({ provider: event.target.value || undefined })
              }
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
              onChange={(event) =>
                patchRuntime({
                  isolation: event.target.value as StudioNode['data']['runtime']['isolation'],
                })
              }
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
              onChange={(event) =>
                patchRuntime({ network: event.target.value as 'none' | 'outbound' })
              }
            >
              <option value="none">None</option>
              <option value="outbound">Outbound</option>
            </select>
          </label>
        </div>
      </section>

      <section className="inspector-section">
        <div className="section-heading"><span>03</span>Resource envelope</div>
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
              value={
                node.data.runtime.memoryBytes
                  ? Math.round(node.data.runtime.memoryBytes / 1024 / 1024)
                  : ''
              }
              placeholder="256"
              onChange={(event) =>
                patchRuntime({
                  memoryBytes: event.target.value
                    ? Number(event.target.value) * 1024 * 1024
                    : undefined,
                })
              }
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

      <section className="inspector-section config-section">
        <div className="section-heading"><span>04</span>Node configuration</div>
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

      {evidence && (
        <section className="runtime-evidence" data-testid="runtime-evidence">
          <div className="evidence-heading">
            <span className={`status-pill state-${evidence.state}`}>{evidence.state}</span>
            <strong>Runtime evidence</strong>
          </div>
          <dl>
            <div><dt>Provider</dt><dd>{evidence.providerId}</dd></div>
            <div><dt>Generation</dt><dd>{evidence.generation}</dd></div>
            <div><dt>Unit</dt><dd title={evidence.unitId ?? ''}>{compact(evidence.unitId)}</dd></div>
            <div><dt>Spec</dt><dd title={evidence.specDigest ?? ''}>{compact(evidence.specDigest)}</dd></div>
          </dl>
        </section>
      )}

      {!['start', 'output'].includes(node.data.kind) && (
        <button className="danger-button" type="button" onClick={() => onDelete(node.id)}>
          Delete node
        </button>
      )}
    </aside>
  );
}

function compact(value?: string | null) {
  if (!value) return '—';
  return value.length > 28 ? `${value.slice(0, 15)}…${value.slice(-9)}` : value;
}
