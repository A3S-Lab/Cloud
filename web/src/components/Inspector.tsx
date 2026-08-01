import { useEffect, useState } from 'react';
import type { StudioNode } from '../graph';
import type { RuntimeEvidence } from '../types';
import { nodeKindLabel, statusLabel } from '../localization';
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
      <aside className="node-panel empty-node-panel" aria-label="节点检查器">
        <div className="empty-state-icon"><CursorIcon /></div>
        <h2>尚未选择节点</h2>
        <p>请在画布上选择一个节点进行配置。</p>
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
    } catch {
      setConfigError('JSON 配置格式无效。');
    }
  };

  return (
    <aside className="node-panel" aria-label="节点检查器" data-testid="node-inspector">
      <header className="panel-header node-panel-header">
        <div className="inspector-title">
          <span className={`node-icon kind-${node.data.kind}`}><NodeIcon kind={node.data.kind} /></span>
          <div><span>节点类型 · {nodeKindLabel(node.data.kind)}</span><h2>{node.data.label}</h2></div>
        </div>
        <div className="panel-header-actions">
          {evidence && <span className={`status-pill state-${evidence.state}`}>{statusLabel(evidence.state)}</span>}
          {onClose && (
            <button type="button" className="icon-button" aria-label="关闭节点检查器" onClick={onClose}>
              <CloseIcon />
            </button>
          )}
        </div>
      </header>

      <nav className="panel-tabs" aria-label="节点设置">
        {(['CONFIG', 'RUNTIME', 'EVIDENCE'] as InspectorTab[]).map((tab) => (
          <button
            type="button"
            className={activeTab === tab ? 'active' : ''}
            key={tab}
            onClick={() => setActiveTab(tab)}
          >
            {{ CONFIG: '配置', RUNTIME: '运行环境', EVIDENCE: '执行证据' }[tab]}
            {tab === 'EVIDENCE' && evidence && <span>1</span>}
          </button>
        ))}
      </nav>

      <div className="node-panel-body">
        {activeTab === 'CONFIG' && (
          <>
            <section className="inspector-section">
              <div className="section-heading"><span>常规</span><small>节点标识</small></div>
              <label>
                显示名称
                <input
                  aria-label="显示名称"
                  value={node.data.label}
                  onChange={(event) => patch({ label: event.target.value })}
                />
              </label>
              <div className="readonly-field"><span>节点 ID</span><code>{node.id}</code></div>
            </section>

            <section className="inspector-section config-section">
              <div className="section-heading"><span>节点配置</span><small>类型化 JSON</small></div>
              <textarea
                aria-label="节点配置 JSON"
                value={configSource}
                onChange={(event) => setConfigSource(event.target.value)}
                spellCheck={false}
              />
              {configError && <p className="field-error">{configError}</p>}
              <button className="secondary-button full" type="button" onClick={applyConfig}>
                应用 JSON
              </button>
            </section>
          </>
        )}

        {activeTab === 'RUNTIME' && (
          <>
            <div className="runtime-callout">
              <RuntimeIcon />
              <div><strong>通过 A3S Runtime 运行</strong><span>运行位置独立于工作流控制平面。</span></div>
            </div>

            <section className="inspector-section">
              <div className="section-heading"><span>运行位置</span><small>提供方与资源池</small></div>
              <div className="field-grid">
                <label>
                  提供方
                  <input
                    aria-label="Runtime 提供方"
                    placeholder="默认"
                    value={node.data.runtime.provider ?? ''}
                    onChange={(event) => patchRuntime({ provider: event.target.value || undefined })}
                  />
                </label>
                <label>
                  资源池
                  <input
                    aria-label="Runtime 资源池"
                    placeholder="cpu / gpu"
                    value={node.data.runtime.pool ?? ''}
                    onChange={(event) => patchRuntime({ pool: event.target.value || undefined })}
                  />
                </label>
              </div>
              <div className="field-grid">
                <label>
                  隔离方式
                  <select
                    aria-label="Runtime 隔离方式"
                    value={node.data.runtime.isolation ?? 'process'}
                    onChange={(event) => patchRuntime({
                      isolation: event.target.value as StudioNode['data']['runtime']['isolation'],
                    })}
                  >
                    <option value="process">进程</option>
                    <option value="container">容器</option>
                    <option value="sandbox">沙箱</option>
                    <option value="confidential">机密计算</option>
                  </select>
                </label>
                <label>
                  网络访问
                  <select
                    aria-label="Runtime 网络访问"
                    value={node.data.runtime.network ?? (['llm', 'agent', 'tool', 'memory', 'http'].includes(node.data.kind) ? 'outbound' : 'none')}
                    onChange={(event) => patchRuntime({ network: event.target.value as 'none' | 'outbound' })}
                  >
                    <option value="none">禁止联网</option>
                    <option value="outbound">允许出站</option>
                  </select>
                </label>
              </div>
            </section>

            <section className="inspector-section">
              <div className="section-heading"><span>资源配额</span><small>每次执行</small></div>
              <div className="resource-grid">
                <label>
                  CPU · 毫核
                  <input
                    type="number"
                    min="1"
                    value={node.data.runtime.cpuMillis ?? ''}
                    placeholder="500"
                    onChange={(event) => patchRuntime({ cpuMillis: parseNumber(event.target.value) })}
                  />
                </label>
                <label>
                  内存 · MiB
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
                  超时 · ms
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
                <span className={`status-pill state-${evidence.state}`}>{statusLabel(evidence.state)}</span>
                <strong>已验证的 Runtime 执行</strong>
              </div>
              <dl>
                <div><dt>提供方</dt><dd>{evidence.providerId}</dd></div>
                <div><dt>资源池</dt><dd>{evidence.runtimePool ?? '默认'}</dd></div>
                <div><dt>代次</dt><dd>{evidence.generation ?? '—'}</dd></div>
                <div><dt>运行单元</dt><dd title={evidence.unitId ?? ''}>{compact(evidence.unitId)}</dd></div>
                <div><dt>规格摘要</dt><dd title={evidence.specDigest ?? ''}>{compact(evidence.specDigest)}</dd></div>
              </dl>
            </section>
          ) : (
            <div className="evidence-empty">
              <RuntimeIcon />
              <h3>暂无 Runtime 执行证据</h3>
              <p>运行工作流后可查看提供方调度、单元代次和内容摘要。</p>
            </div>
          )
        )}
      </div>

      {!['start', 'output'].includes(node.data.kind) && (
        <footer className="node-panel-footer">
          <button className="danger-button" type="button" onClick={() => onDelete(node.id)}>
            <TrashIcon /> 删除节点
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
