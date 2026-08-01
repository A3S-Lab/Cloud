import type { BuildRun } from '../../types/api';
import { humanize, shortId } from '../console/console-format';
import { LogPanel } from './log-panel';

interface BuildRunLogPanelProps {
  buildRun: BuildRun | null;
}

export function BuildRunLogPanel({ buildRun }: BuildRunLogPanelProps) {
  return (
    <LogPanel
      ariaLabel='Build log availability'
      eyebrow='A3S Box contract pending'
      title='Build logs'
      available={false}
      contextLabel={
        buildRun ? `Build ${shortId(buildRun.id)} · ${humanize(buildRun.status)}` : 'No selected build'
      }
      unavailableMessage={
        buildRun
          ? 'Build logs are unavailable until A3S Box exposes an authoritative durable log contract.'
          : 'Select a build run to inspect log availability.'
      }
      records={[]}
      state='idle'
      error={null}
      filter='all'
      onFilterChange={ignoreFilterChange}
    />
  );
}

function ignoreFilterChange() {}
