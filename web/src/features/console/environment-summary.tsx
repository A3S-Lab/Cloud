import { Activity, Bot, Box, Braces, Database, Server, Sparkles } from 'lucide-react';
import type { ReactNode } from 'react';
import type { Deployment, Environment, Organization, Project, Route } from '../../types/api';
import { shortId } from './console-format';

export function EnvironmentHeading({
  organization,
  project,
  environment,
  activeOperations,
  workloadCount,
}: {
  organization: Organization | undefined;
  project: Project | undefined;
  environment: Environment | undefined;
  activeOperations: number;
  workloadCount: number;
}) {
  return (
    <section className='environment-heading'>
      <div>
        <p className='eyebrow'>Observed workspace</p>
        <h1>{environment?.name ?? project?.name ?? organization?.name ?? 'Cloud'}</h1>
        <p>
          {environment
            ? `${organization?.name} / ${project?.name} / ${environment.name}`
            : 'Choose a project and environment to inspect its desired state.'}
        </p>
      </div>
      <div className='heading-facts'>
        <span>
          <Activity size={15} /> {activeOperations} active operation
          {activeOperations === 1 ? '' : 's'}
        </span>
        <span>
          <Box size={15} /> {workloadCount} workload{workloadCount === 1 ? '' : 's'}
        </span>
        <span>
          <Database size={15} /> desired state authoritative
        </span>
      </div>
    </section>
  );
}

export function InfrastructureCard({
  deployment,
  routes,
}: {
  deployment: Deployment | undefined;
  routes: Route[];
}) {
  return (
    <article className='surface infrastructure-card'>
      <div className='surface-heading'>
        <div>
          <p className='eyebrow'>Execution boundary</p>
          <h2>Infrastructure</h2>
        </div>
        <Server size={20} />
      </div>
      <dl className='fact-list'>
        <div>
          <dt>Runtime</dt>
          <dd>Task + Service</dd>
        </div>
        <div>
          <dt>Operation authority</dt>
          <dd>A3S Flow</dd>
        </div>
        <div>
          <dt>Node</dt>
          <dd>{deployment?.nodeId ? shortId(deployment.nodeId) : 'Not scheduled'}</dd>
        </div>
        <div>
          <dt>Edge</dt>
          <dd>
            {routes.length === 0
              ? 'No route projection'
              : `${routes.filter((route) => route.state === 'active').length}/${routes.length} active`}
          </dd>
        </div>
      </dl>
    </article>
  );
}

export function AssetCatalogCard() {
  return (
    <article className='surface assets-card'>
      <div className='surface-heading'>
        <div>
          <p className='eyebrow'>Release catalog</p>
          <h2>A3S assets</h2>
        </div>
        <Sparkles size={20} />
      </div>
      <div className='asset-kinds'>
        <AssetKind icon={<Bot size={18} />} name='Agent' />
        <AssetKind icon={<Braces size={18} />} name='MCP' />
        <AssetKind icon={<Box size={18} />} name='Skill' />
      </div>
      <p className='surface-note'>Immutable releases will use the common workload and deployment path.</p>
    </article>
  );
}

function AssetKind({ icon, name }: { icon: ReactNode; name: string }) {
  return (
    <div>
      <span>{icon}</span>
      <strong>{name}</strong>
      <small>No releases</small>
    </div>
  );
}
