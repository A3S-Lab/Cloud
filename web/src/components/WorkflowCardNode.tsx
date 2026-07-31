import { Handle, Position, type NodeProps } from '@xyflow/react';
import type { StudioNode } from '../graph';
import { NodeIcon } from './NodeIcon';

const category: Record<StudioNode['data']['kind'], string> = {
  start: 'Trigger',
  template: 'Transform',
  llm: 'Intelligence',
  agent: 'Intelligence',
  tool: 'Action',
  router: 'Control',
  memory: 'Context',
  http: 'Action',
  approval: 'Human',
  output: 'Result',
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

export function WorkflowCardNode({ id, data, selected }: NodeProps<StudioNode>) {
  const routes = data.kind === 'router' ? routerHandles(data.config) : [];
  const runtime = [data.runtime.provider ?? 'default', data.runtime.pool]
    .filter(Boolean)
    .join(' / ');
  return (
    <article
      className={`workflow-node kind-${data.kind}${selected ? ' selected' : ''}`}
      data-testid={`workflow-node-${id}`}
      aria-label={`${data.label} ${data.kind} node`}
    >
      {data.kind !== 'start' && (
        <Handle type="target" position={Position.Left} className="node-handle" />
      )}
      <div className="node-accent" />
      <div className="node-header">
        <span className="node-icon">
          <NodeIcon kind={data.kind} />
        </span>
        <span className="node-category">{category[data.kind]}</span>
        {data.executionState && (
          <span className={`execution-dot state-${data.executionState}`} title={data.executionState} />
        )}
      </div>
      <strong>{data.label}</strong>
      <div className="node-runtime">
        <span>RT</span>
        <span>{runtime}</span>
      </div>
      {data.kind === 'router' ? (
        <div className="route-handles" aria-label="Router outputs">
          {routes.map((route, index) => (
            <span key={route} className="route-label">
              {route}
              <Handle
                id={route}
                type="source"
                position={Position.Right}
                className="node-handle route-handle"
                style={{ top: `${38 + index * 18}%` }}
              />
            </span>
          ))}
        </div>
      ) : (
        data.kind !== 'output' && (
          <Handle type="source" position={Position.Right} className="node-handle" />
        )
      )}
    </article>
  );
}
