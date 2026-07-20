import { Box } from 'lucide-react';
import type { Environment, Workload } from '../../types/api';

export function WorkloadList({
  workloads,
  selectedWorkloadId,
  environment,
  onSelect,
}: {
  workloads: Workload[];
  selectedWorkloadId: string;
  environment: Environment | undefined;
  onSelect: (workloadId: string) => void;
}) {
  return (
    <section className='workload-section' aria-label='Workloads'>
      <div className='section-heading'>
        <div>
          <p className='eyebrow'>Desired and observed state</p>
          <h2>Workloads</h2>
        </div>
        <span>{environment ? environment.name : 'Select an environment'}</span>
      </div>
      {workloads.length === 0 ? (
        <div className='surface workload-empty'>
          <Box size={22} />
          <strong>No workloads in this environment</strong>
          <p>Create a digest-bound Service deployment to start convergence.</p>
        </div>
      ) : (
        <div className='workload-list'>
          {workloads.map((workload) => {
            const deployment = workload.deployments[0];
            return (
              <button
                className={workload.id === selectedWorkloadId ? 'workload-row selected' : 'workload-row'}
                type='button'
                key={workload.id}
                onClick={() => onSelect(workload.id)}
              >
                <span className={`workload-state ${deployment?.status ?? 'neutral'}`} />
                <span className='workload-identity'>
                  <strong>{workload.name}</strong>
                  <small>
                    {workload.desiredRevision?.artifactUri ??
                      workload.desiredRevision?.artifactSourceUri ??
                      'No desired revision'}
                  </small>
                </span>
                <span>
                  <small>Desired</small>
                  <strong>{revisionLabel(workload.desiredRevision)}</strong>
                </span>
                <span>
                  <small>Observed</small>
                  <strong>
                    {deployment?.observedRuntime
                      ? `${deployment.observedRuntime.state} · ${deployment.observedRuntime.healthState ?? 'no health'}`
                      : 'No evidence'}
                  </strong>
                </span>
                <span>
                  <small>Operation</small>
                  <strong>{deployment?.operation?.status ?? deployment?.status ?? 'queued'}</strong>
                </span>
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}

function revisionLabel(revision: Workload['desiredRevision']): string {
  return revision ? `Generation ${revision.generation}` : 'None';
}
