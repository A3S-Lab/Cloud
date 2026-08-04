import { Activity, Bot, Box, Braces, Database, Server, Sparkles } from 'lucide-react';
import type { ReactNode } from 'react';
import type {
  Asset,
  AssetKind as AssetKindValue,
  AssetRelease,
  Deployment,
  Environment,
  Organization,
  Project,
  Route,
} from '../../types/api';
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

export function AssetCatalogCard({ assets, releases }: { assets: Asset[]; releases: AssetRelease[] }) {
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
        <AssetKind assets={assets} releases={releases} icon={<Bot size={18} />} kind='agent' name='Agent' />
        <AssetKind assets={assets} releases={releases} icon={<Braces size={18} />} kind='mcp' name='MCP' />
        <AssetKind assets={assets} releases={releases} icon={<Box size={18} />} kind='skill' name='Skill' />
      </div>
      <p className='surface-note'>
        Published Agent releases deploy through immutable Workload bindings. Yanked releases remain available
        to pinned deployments.
      </p>
    </article>
  );
}

function AssetKind({
  assets,
  releases,
  icon,
  kind,
  name,
}: {
  assets: Asset[];
  releases: AssetRelease[];
  icon: ReactNode;
  kind: AssetKindValue;
  name: string;
}) {
  const matchingAssets = assets.filter((asset) => asset.kind === kind);
  const assetIds = new Set(matchingAssets.map((asset) => asset.id));
  const matchingReleases = releases.filter((release) => assetIds.has(release.assetId));
  const published = matchingReleases.filter((release) => release.state === 'published').length;
  const draft = matchingReleases.filter((release) => release.state === 'draft').length;
  const yanked = matchingReleases.filter((release) => release.state === 'yanked').length;
  return (
    <div>
      <span>{icon}</span>
      <strong>{name}</strong>
      <small>
        {matchingAssets.length} asset{matchingAssets.length === 1 ? '' : 's'} · {published} published
      </small>
      <small>
        {draft} draft · {yanked} yanked
      </small>
    </div>
  );
}
