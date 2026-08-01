import { Handle, Position, type NodeProps } from '@xyflow/react';
import type { StudioNode } from '../graph';
import { nodeKindDescription, nodeKindLabel, statusLabel } from '../localization';
import { NodeIcon } from './NodeIcon';

const category: Record<StudioNode['data']['kind'], string> = {
  start: '触发器',
  template: '转换',
  llm: 'AI',
  agent: '智能体',
  tool: '工具',
  router: '逻辑',
  memory: '上下文',
  http: '实用工具',
  approval: '人工输入',
  output: '结束',
};

function routerHandles(config: unknown): string[] {
  if (!config || typeof config !== 'object') return ['default'];
  const value = config as { routes?: Array<{ route?: unknown }>; default?: unknown };
  const routes = (value.routes ?? [])
    .map((item) => item.route)
    .filter((route): route is string => typeof route === 'string' && route.length > 0);
  if (typeof value.default === 'string' && value.default.length > 0) {
    routes.push(value.default);
  }
  return [...new Set(routes.length ? routes : ['default'])];
}

function configLabel(data: StudioNode['data'], routes: string[]): string {
  const config = data.config && typeof data.config === 'object'
    ? data.config as Record<string, unknown>
    : {};
  if (data.kind === 'llm' || data.kind === 'agent') {
    const model = config.model ?? config.model_name;
    return typeof model === 'string' && model ? model : 'Runtime 模型';
  }
  if (data.kind === 'router') return `${routes.length} 个分支`;
  if (data.kind === 'template') return '模板转换';
  if (data.kind === 'start') return '用户输入';
  if (data.kind === 'output') return '输出变量';
  if (data.kind === 'approval') return '审批请求';
  return category[data.kind];
}

export function WorkflowCardNode({ id, data, selected }: NodeProps<StudioNode>) {
  const routes = data.kind === 'router' ? routerHandles(data.config) : [];
  return (
    <article
      className={`workflow-node kind-${data.kind}${selected ? ' selected' : ''}`}
      data-testid={`workflow-node-${id}`}
      aria-label={`${data.label} · ${nodeKindLabel(data.kind)}节点`}
    >
      {data.kind !== 'start' && (
        <Handle type="target" position={Position.Left} className="node-handle" style={{ top: 24 }} />
      )}
      <div className="node-header">
        <span className={`node-icon kind-${data.kind}`}>
          <NodeIcon kind={data.kind} />
        </span>
        <span className="node-title"><strong>{data.label}</strong></span>
        {data.executionState ? (
          <span
            className={`node-execution-dot state-${data.executionState}`}
            title={statusLabel(data.executionState)}
          >
            {data.executionState === 'succeeded' ? '✓' : data.executionState === 'running' ? '…' : '!'}
          </span>
        ) : (
          <span className="node-menu" aria-hidden="true">•••</span>
        )}
      </div>

      <div className="node-config-line">
        <span className={`config-glyph kind-${data.kind}`}><NodeIcon kind={data.kind} size={12} /></span>
        <strong>{configLabel(data, routes)}</strong>
        {(data.kind === 'llm' || data.kind === 'agent') && <small>对话</small>}
      </div>
      <p className="node-description">{nodeKindDescription(data.kind)}</p>

      {data.kind === 'router' ? (
        <div className="route-handles" aria-label="条件分支输出">
          {routes.map((route, index) => (
            <span key={route} className="route-label">
              {route}
              <Handle
                id={route}
                type="source"
                position={Position.Right}
                className="node-handle route-handle"
                style={{ top: `${42 + index * 16}%` }}
              />
            </span>
          ))}
        </div>
      ) : (
        data.kind !== 'output' && (
          <Handle type="source" position={Position.Right} className="node-handle" style={{ top: 24 }} />
        )
      )}
    </article>
  );
}
