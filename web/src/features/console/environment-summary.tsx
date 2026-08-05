import { Activity, Bot, Box, Braces, Database, Server, Sparkles } from 'lucide-react';
import type { ReactNode } from 'react';
import { useI18n } from '../../lib/i18n';
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
  const { t } = useI18n();
  const workspaceName = environment?.name ?? project?.name ?? organization?.name ?? 'Cloud';
  return (
    <section className='environment-heading'>
      <div className='environment-title'>
        <h1>{t('{name} workspace', { name: workspaceName })}</h1>
        <p>
          {environment
            ? `${organization?.name} / ${project?.name} / ${environment.name}`
            : t('Choose a project and environment to inspect its desired state.')}
        </p>
      </div>
      <dl className='heading-facts'>
        <div>
          <dt>
            <Activity size={16} /> {t('Operations')}
          </dt>
          <dd>{t('{count} active', { count: activeOperations })}</dd>
        </div>
        <div>
          <dt>
            <Box size={16} /> {t('Workloads')}
          </dt>
          <dd>{t('{count} observed', { count: workloadCount })}</dd>
        </div>
        <div>
          <dt>
            <Database size={16} /> {t('Desired state')}
          </dt>
          <dd>{t('Authoritative')}</dd>
        </div>
      </dl>
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
  const { t } = useI18n();
  return (
    <article className='surface infrastructure-card'>
      <div className='surface-heading'>
        <div>
          <h2>{t('Infrastructure')}</h2>
          <p>{t('Current control and execution ownership')}</p>
        </div>
        <Server size={20} />
      </div>
      <dl className='fact-list'>
        <div>
          <dt>{t('Runtime')}</dt>
          <dd>Task + Service</dd>
        </div>
        <div>
          <dt>{t('Operation authority')}</dt>
          <dd>A3S Flow</dd>
        </div>
        <div>
          <dt>{t('Node')}</dt>
          <dd>{deployment?.nodeId ? shortId(deployment.nodeId) : t('Not scheduled')}</dd>
        </div>
        <div>
          <dt>{t('Edge')}</dt>
          <dd>
            {routes.length === 0
              ? t('No route projection')
              : t('{active}/{total} active', {
                  active: routes.filter((route) => route.state === 'active').length,
                  total: routes.length,
                })}
          </dd>
        </div>
      </dl>
    </article>
  );
}

export function AssetCatalogCard({ assets, releases }: { assets: Asset[]; releases: AssetRelease[] }) {
  const { t } = useI18n();
  return (
    <article className='surface assets-card'>
      <div className='surface-heading'>
        <div>
          <h2>{t('A3S assets')}</h2>
          <p>{t('Immutable Agent, MCP, and Skill releases')}</p>
        </div>
        <Sparkles size={20} />
      </div>
      <div className='asset-kinds'>
        <AssetKind assets={assets} releases={releases} icon={<Bot size={18} />} kind='agent' name='Agent' />
        <AssetKind assets={assets} releases={releases} icon={<Braces size={18} />} kind='mcp' name='MCP' />
        <AssetKind assets={assets} releases={releases} icon={<Box size={18} />} kind='skill' name='Skill' />
      </div>
      <p className='surface-note'>
        {t(
          'Published Agent releases deploy through immutable Workload bindings. Yanked releases remain available to pinned deployments.'
        )}
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
  const { t } = useI18n();
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
        {t(matchingAssets.length === 1 ? '{count} asset' : '{count} assets', {
          count: matchingAssets.length,
        })}{' '}
        · {t('{count} published', { count: published })}
      </small>
      <small>
        {t('{count} draft', { count: draft })} · {t('{count} yanked', { count: yanked })}
      </small>
    </div>
  );
}
