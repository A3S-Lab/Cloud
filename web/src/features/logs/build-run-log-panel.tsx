import type { BuildRun } from '../../types/api';
import { useI18n } from '../../lib/i18n';
import { shortId } from '../console/console-format';
import { LogPanel } from './log-panel';

interface BuildRunLogPanelProps {
  buildRun: BuildRun | null;
}

export function BuildRunLogPanel({ buildRun }: BuildRunLogPanelProps) {
  const { label, t } = useI18n();
  return (
    <LogPanel
      ariaLabel={t('Build log availability')}
      eyebrow={t('A3S Box contract pending')}
      title={t('Build logs')}
      available={false}
      contextLabel={
        buildRun
          ? t('Build {id} · {status}', { id: shortId(buildRun.id), status: label(buildRun.status) })
          : t('No selected build')
      }
      unavailableMessage={
        buildRun
          ? t('Build logs are unavailable until A3S Box exposes an authoritative durable log contract.')
          : t('Select a build run to inspect log availability.')
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
