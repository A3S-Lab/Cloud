import { useState } from 'react';
import type { CloudApi } from '../../lib/api';
import type { WorkloadLogStreamFilter } from '../../types/api';
import { LogPanel } from './log-panel';
import { useWorkloadLogStream } from './use-workload-log-stream';

type LogFilter = 'all' | WorkloadLogStreamFilter;

interface LiveLogPanelProps {
  api: CloudApi;
  organizationId: string | null;
  workloadId: string | null;
  revisionId: string | null;
  generation: number | null;
}

export function LiveLogPanel({ api, organizationId, workloadId, revisionId, generation }: LiveLogPanelProps) {
  const [filter, setFilter] = useState<LogFilter>('all');
  const stream = useWorkloadLogStream(
    api,
    organizationId,
    workloadId,
    revisionId,
    filter === 'all' ? undefined : filter
  );

  return (
    <LogPanel
      ariaLabel='Live workload logs'
      eyebrow='Bounded live delivery'
      title='Workload logs'
      available={revisionId !== null}
      contextLabel={generation ? `Generation ${generation}` : 'No active revision'}
      unavailableMessage='Logs become available after a revision is scheduled.'
      records={stream.records}
      state={stream.state}
      error={stream.error}
      filter={filter}
      onFilterChange={setFilter}
    />
  );
}
