import { Clock3, GitCommitHorizontal } from 'lucide-react';
import type { Operation, Workload } from '../../types/api';
import { formatTimestamp, humanize, shortId } from './console-format';

interface DeploymentTimelineProps {
  workload: Workload | undefined;
  operations: Operation[];
}

export function DeploymentTimeline({ workload, operations }: DeploymentTimelineProps) {
  const deployments = [...(workload?.deployments ?? [])].sort(
    (left, right) => Date.parse(right.requestedAt) - Date.parse(left.requestedAt)
  );

  return (
    <section className='surface deployment-timeline' aria-label='Deployment timeline'>
      <div className='surface-heading'>
        <div>
          <p className='eyebrow'>Immutable history</p>
          <h2>Deployment timeline</h2>
        </div>
        <span className='panel-count'>
          <Clock3 size={14} /> {deployments.length}
        </span>
      </div>
      {deployments.length === 0 ? (
        <div className='detail-empty'>
          <GitCommitHorizontal size={21} />
          <strong>No deployment projection</strong>
          <p>Committed generations appear here with their observed operation state.</p>
        </div>
      ) : (
        <ol className='deployment-timeline-list'>
          {deployments.map((deployment) => {
            const operation = operations.find((item) => item.id === deployment.operationId);
            const rollbackSource = operation?.rollbackSourceRevisionId
              ? workload?.deployments.find((item) => item.revision.id === operation.rollbackSourceRevisionId)
                  ?.revision
              : undefined;
            const isCurrent = workload?.activeRevision?.id === deployment.revision.id;
            return (
              <li key={deployment.id}>
                <span className={`timeline-marker ${deployment.status}`} />
                <article>
                  <div className='timeline-title'>
                    <div>
                      <strong>Generation {deployment.revision.generation}</strong>
                      {isCurrent ? <span className='current-label'>Current</span> : null}
                    </div>
                    <span className={`state-badge ${deployment.status}`}>{humanize(deployment.status)}</span>
                  </div>
                  <p className='timeline-artifact'>
                    {deployment.revision.artifactUri ?? deployment.revision.artifactSourceUri}
                  </p>
                  {operation?.rollbackSourceRevisionId ? (
                    <p className='timeline-lineage'>
                      Rollback from{' '}
                      {rollbackSource
                        ? `generation ${rollbackSource.generation}`
                        : shortId(operation.rollbackSourceRevisionId)}
                    </p>
                  ) : null}
                  {deployment.revision.externalSourceRevisionId && deployment.revision.buildRunId ? (
                    <p className='timeline-lineage'>
                      Source {shortId(deployment.revision.externalSourceRevisionId)} · build{' '}
                      {shortId(deployment.revision.buildRunId)}
                    </p>
                  ) : null}
                  {deployment.revision.agentBinding ? (
                    <p className='timeline-lineage'>
                      Agent release {shortId(deployment.revision.agentBinding.assetReleaseId)} / build{' '}
                      {shortId(deployment.revision.agentBinding.buildRunId)}
                    </p>
                  ) : null}
                  {deployment.revision.mcpBinding ? (
                    <p className='timeline-lineage'>
                      MCP release {shortId(deployment.revision.mcpBinding.assetReleaseId)} / profile{' '}
                      {deployment.revision.mcpBinding.profileDigest.slice(0, 15)}
                    </p>
                  ) : null}
                  {deployment.revision.skillBindings.length > 0 ? (
                    <p className='timeline-lineage'>
                      Skills{' '}
                      {deployment.revision.skillBindings
                        .map((binding) => shortId(binding.assetReleaseId))
                        .join(', ')}
                    </p>
                  ) : null}
                  <dl className='timeline-facts'>
                    <div>
                      <dt>Requested</dt>
                      <dd>{formatTimestamp(deployment.requestedAt)}</dd>
                    </div>
                    <div>
                      <dt>Activated</dt>
                      <dd>{formatTimestamp(deployment.activatedAt)}</dd>
                    </div>
                    <div>
                      <dt>Node</dt>
                      <dd>{deployment.nodeId ? shortId(deployment.nodeId) : 'Not scheduled'}</dd>
                    </div>
                    <div>
                      <dt>Operation</dt>
                      <dd>
                        {humanize(operation?.status ?? deployment.operation?.status ?? deployment.status)}
                      </dd>
                    </div>
                  </dl>
                  {deployment.failure || operation?.error || deployment.operation?.error ? (
                    <output className='timeline-failure'>
                      {deployment.failure ?? operation?.error ?? deployment.operation?.error}
                    </output>
                  ) : null}
                </article>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
