import { Activity, Boxes, GitBranch, Route as RouteIcon } from 'lucide-react';
import type { CloudApi } from '../../lib/api';
import type {
  BuildRun,
  Deployment,
  Environment,
  GatewayCertificate,
  Operation,
  Route,
  ServiceTemplate,
  Workload,
} from '../../types/api';
import { BuildRunLogPanel } from '../logs/build-run-log-panel';
import { LiveLogPanel } from '../logs/live-log-panel';
import { BuildEvidencePanel } from './build-evidence-panel';
import { BuildRunPanel } from './build-run-panel';
import { DeploymentTimeline } from './deployment-timeline';
import { EdgeStatusPanel } from './edge-status-panel';
import { AssetCatalogCard, InfrastructureCard } from './environment-summary';
import { WorkloadList } from './workload-list';
import { WorkloadOverview } from './workload-overview';

interface OverviewSectionProps {
  activeOperations: number;
  buildRunCount: number;
  deployment: Deployment | undefined;
  routes: Route[];
  workloadCount: number;
}

export function OverviewSection({
  activeOperations,
  buildRunCount,
  deployment,
  routes,
  workloadCount,
}: OverviewSectionProps) {
  const activeRoutes = routes.filter((route) => route.state === 'active').length;
  return (
    <section
      id='console-overview-panel'
      className='console-section overview-section'
      role='tabpanel'
      aria-labelledby='console-overview-tab'
    >
      <div className='overview-grid'>
        <EnvironmentActivityCard
          activeOperations={activeOperations}
          activeRoutes={activeRoutes}
          buildRunCount={buildRunCount}
          workloadCount={workloadCount}
        />
        <InfrastructureCard deployment={deployment} routes={routes} />
        <AssetCatalogCard />
      </div>
    </section>
  );
}

function EnvironmentActivityCard({
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
  return (
    <article className='surface activity-card'>
      <div className='surface-heading'>
        <div>
          <p className='eyebrow'>Current projection</p>
          <h2>Environment activity</h2>
        </div>
        <Activity size={20} />
      </div>
      <dl className='activity-facts'>
        <div>
          <dt>
            <Boxes size={15} /> Workloads
          </dt>
          <dd>{workloadCount}</dd>
        </div>
        <div>
          <dt>
            <Activity size={15} /> Active operations
          </dt>
          <dd>{activeOperations}</dd>
        </div>
        <div>
          <dt>
            <GitBranch size={15} /> Build runs
          </dt>
          <dd>{buildRunCount}</dd>
        </div>
        <div>
          <dt>
            <RouteIcon size={15} /> Active routes
          </dt>
          <dd>{activeRoutes}</dd>
        </div>
      </dl>
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
}

export function WorkloadsSection({
  api,
  organizationId,
  environment,
  workloads,
  workload,
  routes,
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
