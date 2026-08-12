import {
  Activity,
  Box,
  Boxes,
  CheckCircle2,
  CloudCog,
  Code2,
  Network,
  Route as RouteIcon,
  Server,
  Workflow,
} from 'lucide-react';
import type { CloudApi } from '../../lib/api';
import { useI18n } from '../../lib/i18n';
import type {
  Asset,
  AssetRelease,
  AgentConversation,
  BuildRun,
  Deployment,
  Environment,
  GatewayCertificate,
  Operation,
  Route,
  ServiceTemplate,
  Workload,
} from '../../types/api';
import { AgentExecutionPanel } from '../agents/agent-execution-panel';
import { BuildRunLogPanel } from '../logs/build-run-log-panel';
import { LiveLogPanel } from '../logs/live-log-panel';
import { BuildEvidencePanel } from './build-evidence-panel';
import { BuildRunPanel } from './build-run-panel';
import { statusBadgeState } from './console-format';
import { DeploymentTimeline } from './deployment-timeline';
import { EdgeStatusPanel } from './edge-status-panel';
import { AssetCatalogCard, InfrastructureCard } from './environment-summary';
import { SkillBindingsPanel } from './skill-bindings-panel';
import { WorkloadList } from './workload-list';
import { WorkloadOverview } from './workload-overview';
import { isTerminalOperation } from './workload-view-model';

interface OverviewSectionProps {
  operations: Operation[];
  assets: Asset[];
  assetReleases: AssetRelease[];
  buildRunCount: number;
  deployment: Deployment | undefined;
  routes: Route[];
  workloadCount: number;
}

export function OverviewSection({
  operations,
  assets,
  assetReleases,
  buildRunCount,
  deployment,
  routes,
  workloadCount,
}: OverviewSectionProps) {
  const activeOperations = operations.filter((operation) => !isTerminalOperation(operation)).length;
  const activeRoutes = routes.filter((route) => route.state === 'active').length;
  return (
    <section
      id='console-overview-panel'
      className='console-section overview-section'
      role='tabpanel'
      aria-labelledby='console-overview-tab'
    >
      <OverviewStatusBand
        activeOperations={activeOperations}
        activeRoutes={activeRoutes}
        buildRunCount={buildRunCount}
        workloadCount={workloadCount}
      />
      <div className='overview-grid'>
        <CurrentOperationsCard operations={operations} />
        <AuthorityChain />
      </div>
      <div className='overview-support-grid'>
        <AssetCatalogCard assets={assets} releases={assetReleases} />
        <InfrastructureCard deployment={deployment} routes={routes} />
      </div>
    </section>
  );
}

function OverviewStatusBand({
  activeOperations,
  activeRoutes,
  buildRunCount,
  workloadCount,
}: {
  activeOperations: number;
  activeRoutes: number;
  buildRunCount: number;
  workloadCount: number;
}) {
  const { t } = useI18n();
  const converged = activeOperations === 0;
  const StatusIcon = converged ? CheckCircle2 : Activity;
  return (
    <article className='overview-status-band'>
      <div className='overview-status-copy'>
        <span aria-hidden='true'>
          <StatusIcon size={22} />
        </span>
        <div>
          <strong>{converged ? t('Desired state is converged') : t('Convergence is in progress')}</strong>
          <small>
            {converged
              ? t('No active operation is changing the selected environment.')
              : t(
                  activeOperations === 1
                    ? '{count} durable operation currently active.'
                    : '{count} durable operations currently active.',
                  { count: activeOperations }
                )}
          </small>
        </div>
      </div>
      <dl className='property-list overview-status-facts' data-variant='plain'>
        <div>
          <dt>{t('Workloads')}</dt>
          <dd>{workloadCount}</dd>
        </div>
        <div>
          <dt>{t('Active operations')}</dt>
          <dd>{activeOperations}</dd>
        </div>
        <div>
          <dt>{t('Build runs')}</dt>
          <dd>{buildRunCount}</dd>
        </div>
        <div>
          <dt>{t('Active routes')}</dt>
          <dd>{activeRoutes}</dd>
        </div>
      </dl>
    </article>
  );
}

function CurrentOperationsCard({ operations }: { operations: Operation[] }) {
  const { formatRelative, label, t } = useI18n();
  const recent = operations.slice(0, 5);
  return (
    <article className='card surface current-operations-card' data-size='sm'>
      <header className='surface-heading'>
        <div>
          <h2>{t('Current operations')}</h2>
          <p>{t('Latest durable workflow state for this organization')}</p>
        </div>
        <span className='badge card-action' data-variant='secondary'>
          {t('{count} total', { count: operations.length })}
        </span>
      </header>
      <section>
        {recent.length === 0 ? (
          <div className='empty overview-empty-state'>
            <figure>
              <CheckCircle2 size={22} />
            </figure>
            <header>
              <h3>{t('No operations recorded')}</h3>
              <p>{t('Accepted mutations and their terminal evidence will appear here.')}</p>
            </header>
          </div>
        ) : (
          <ol className='item-group overview-operation-list'>
            {recent.map((operation) => (
              <li className='item' data-size='sm' data-variant='outline' key={operation.id}>
                <span className={`operation-status ${operation.status}`} aria-hidden='true' />
                <span className='overview-operation-name' data-item-content>
                  <strong>{label(operation.subjectKind)}</strong>
                  <small>
                    {operation.workflowName}@{operation.workflowVersion}
                  </small>
                </span>
                <span
                  className='status-badge'
                  data-state={statusBadgeState(operation.status)}
                  data-size='sm'
                  data-indicator
                >
                  {label(operation.status)}
                </span>
                <time dateTime={operation.updatedAt}>{formatRelative(operation.updatedAt)}</time>
              </li>
            ))}
          </ol>
        )}
      </section>
    </article>
  );
}

const AUTHORITY_LAYERS = [
  { label: 'A3S OS control', detail: 'Intent, identity, and policy', icon: CloudCog },
  { label: 'Operations + A3S Flow', detail: 'Durable orchestration and recovery', icon: Workflow },
  { label: 'Workloads', detail: 'Placement, revisions, and convergence', icon: Boxes },
  { label: 'Outbound-only Node Agent', detail: 'Leases, Claims, commands, and receipts', icon: Network },
  { label: 'A3S Runtime + Box', detail: 'Task, Service, build, and isolation', icon: Server },
  { label: 'A3S Gateway', detail: 'Applied request-path policy', icon: RouteIcon },
  { label: 'Agent execution providers', detail: 'One provider-neutral contract', icon: Code2 },
] as const;

function AuthorityChain() {
  const { t } = useI18n();
  return (
    <article className='card surface authority-chain-card' data-size='sm'>
      <header className='surface-heading'>
        <div>
          <h2>{t('Authority and runtime path')}</h2>
          <p>{t('One control route from accepted intent to execution evidence')}</p>
        </div>
        <Box className='card-action' size={20} aria-hidden='true' />
      </header>
      <section>
        <ol className='item-group authority-chain'>
          {AUTHORITY_LAYERS.map(({ label, detail, icon: Icon }) => (
            <li
              className={`item${label === 'Agent execution providers' ? ' authority-harness' : ''}`}
              data-size='sm'
              data-variant={label === 'Agent execution providers' ? 'muted' : 'outline'}
              key={label}
            >
              <figure aria-hidden='true'>
                <Icon size={18} />
              </figure>
              <section>
                <h3>{t(label)}</h3>
                <p>{t(detail)}</p>
              </section>
            </li>
          ))}
        </ol>
      </section>
    </article>
  );
}

interface WorkloadsSectionProps {
  api: CloudApi;
  organizationId: string | null;
  environment: Environment | undefined;
  workloads: Workload[];
  workload: Workload | undefined;
  routes: Route[];
  assets: Asset[];
  assetReleases: AssetRelease[];
  operations: Operation[];
  selectedWorkloadId: string;
  cancelling: boolean;
  stopping: boolean;
  logRevisionId: string | null;
  logGeneration: number | null;
  onSelectWorkload: (workloadId: string) => void;
  onCancel: () => Promise<void>;
  onStop: () => Promise<void>;
  onUpdate: (template: ServiceTemplate, idempotencyKey: string) => Promise<void>;
  onRollback: (revisionId: string, idempotencyKey: string) => Promise<void>;
  onBindSkill: (skillAssetId: string, skillAssetReleaseId: string, idempotencyKey: string) => Promise<void>;
  onUnbindSkill: (skillAssetId: string, idempotencyKey: string) => Promise<void>;
}

export function WorkloadsSection({
  api,
  organizationId,
  environment,
  workloads,
  workload,
  routes,
  assets,
  assetReleases,
  operations,
  selectedWorkloadId,
  cancelling,
  stopping,
  logRevisionId,
  logGeneration,
  onSelectWorkload,
  onCancel,
  onStop,
  onUpdate,
  onRollback,
  onBindSkill,
  onUnbindSkill,
}: WorkloadsSectionProps) {
  return (
    <section
      id='console-workloads-panel'
      className='console-section workloads-section'
      role='tabpanel'
      aria-labelledby='console-workloads-tab'
    >
      <WorkloadList
        workloads={workloads}
        selectedWorkloadId={selectedWorkloadId}
        environment={environment}
        onSelect={onSelectWorkload}
      />
      <WorkloadOverview
        workload={workload}
        routes={routes}
        cancelling={cancelling}
        stopping={stopping}
        onCancel={onCancel}
        onStop={onStop}
        onUpdate={onUpdate}
        onRollback={onRollback}
      />
      <SkillBindingsPanel
        workload={workload}
        assets={assets}
        releases={assetReleases}
        onBind={onBindSkill}
        onUnbind={onUnbindSkill}
      />
      <DeploymentTimeline workload={workload} operations={operations} />
      <LiveLogPanel
        api={api}
        organizationId={organizationId}
        workloadId={workload?.id ?? null}
        revisionId={logRevisionId}
        generation={logGeneration}
      />
    </section>
  );
}

interface AgentSectionProps {
  api: CloudApi;
  organizationId: string | null;
  projectId: string | null;
  environmentId: string | null;
  conversations: AgentConversation[];
  selectedConversationId: string;
  assets: Asset[];
  assetReleases: AssetRelease[];
  onSelectConversation: (conversationId: string) => void;
  onConversationChanged: (conversation: AgentConversation) => void;
  onError: (cause: unknown) => void;
}

export function AgentSection({
  api,
  organizationId,
  projectId,
  environmentId,
  conversations,
  selectedConversationId,
  assets,
  assetReleases,
  onSelectConversation,
  onConversationChanged,
  onError,
}: AgentSectionProps) {
  return (
    <section
      id='console-agents-panel'
      className='console-section agents-section'
      role='tabpanel'
      aria-labelledby='console-agents-tab'
    >
      <AgentExecutionPanel
        api={api}
        organizationId={organizationId}
        projectId={projectId}
        environmentId={environmentId}
        conversations={conversations}
        selectedConversationId={selectedConversationId}
        assets={assets}
        releases={assetReleases}
        onSelectConversation={onSelectConversation}
        onConversationChanged={onConversationChanged}
        onError={onError}
      />
    </section>
  );
}

interface DeliverySectionProps {
  api: CloudApi;
  organizationId: string | null;
  buildRuns: BuildRun[];
  selectedBuildRun: BuildRun | null;
  selectedBuildRunId: string | null;
  cancellingBuildRunId: string | null;
  retryingBuildRunId: string | null;
  onSelect: (buildRunId: string) => void;
  onCancel: (buildRunId: string) => void;
  onRetry: (buildRunId: string) => void;
}

export function DeliverySection({
  api,
  organizationId,
  buildRuns,
  selectedBuildRun,
  selectedBuildRunId,
  cancellingBuildRunId,
  retryingBuildRunId,
  onSelect,
  onCancel,
  onRetry,
}: DeliverySectionProps) {
  return (
    <section
      id='console-delivery-panel'
      className='console-section delivery-section'
      role='tabpanel'
      aria-labelledby='console-delivery-tab'
    >
      <BuildRunPanel
        buildRuns={buildRuns}
        selectedBuildRunId={selectedBuildRunId}
        cancellingBuildRunId={cancellingBuildRunId}
        retryingBuildRunId={retryingBuildRunId}
        onSelect={onSelect}
        onCancel={onCancel}
        onRetry={onRetry}
      />
      <BuildEvidencePanel api={api} organizationId={organizationId} buildRun={selectedBuildRun} />
      <BuildRunLogPanel buildRun={selectedBuildRun} />
    </section>
  );
}

interface EdgeSectionProps {
  environment: Environment | undefined;
  workloads: Workload[];
  workload: Workload | undefined;
  routes: Route[];
  certificates: GatewayCertificate[];
  selectedWorkloadId: string;
  onSelectWorkload: (workloadId: string) => void;
}

export function EdgeSection({
  environment,
  workloads,
  workload,
  routes,
  certificates,
  selectedWorkloadId,
  onSelectWorkload,
}: EdgeSectionProps) {
  return (
    <section
      id='console-edge-panel'
      className='console-section edge-section'
      role='tabpanel'
      aria-labelledby='console-edge-tab'
    >
      <WorkloadList
        workloads={workloads}
        selectedWorkloadId={selectedWorkloadId}
        environment={environment}
        onSelect={onSelectWorkload}
      />
      <EdgeStatusPanel workload={workload} routes={routes} certificates={certificates} />
    </section>
  );
}
