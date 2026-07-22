import { useState } from 'react';
import type { CloudApi } from '../../lib/api';
import type { BuildRun, WorkloadLogStreamFilter } from '../../types/api';
import { humanize, shortId } from '../console/console-format';
import { LogPanel } from './log-panel';
import { useBuildRunLogStream } from './use-build-run-log-stream';

type LogFilter = 'all' | WorkloadLogStreamFilter;

interface BuildRunLogPanelProps {
  api: CloudApi;
  organizationId: string | null;
  buildRun: BuildRun | null;
}

export function BuildRunLogPanel({ api, organizationId, buildRun }: BuildRunLogPanelProps) {
  const [filter, setFilter] = useState<LogFilter>('all');
  const stream = useBuildRunLogStream(
    api,
    organizationId,
    buildRun?.id ?? null,
    filter === 'all' ? undefined : filter
  );

  return (
    <LogPanel
      ariaLabel='Live build logs'
      eyebrow='BuildKit plain progress'
      title='Build logs'
      available={buildRun !== null}
      contextLabel={
        buildRun ? `Build ${shortId(buildRun.id)} · ${humanize(buildRun.status)}` : 'No selected build'
      }
      unavailableMessage='Select a build run to inspect its ordered Runtime output.'
      records={stream.records}
      state={stream.state}
      error={stream.error}
      filter={filter}
      onFilterChange={setFilter}
    />
  );
}
