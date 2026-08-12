import { LogOut, PanelRightClose, PanelRightOpen } from 'lucide-react';
import type { CloudApi } from '../../lib/api';
import { LanguageSwitcher, useI18n } from '../../lib/i18n';
import type { SearchResult } from '../../types/api';
import type { StreamState } from '../operations/use-operation-stream';
import { ResourceSearch } from '../search/resource-search';
import { statusBadgeState } from './console-format';

interface ConsoleTopbarProps {
  api: CloudApi;
  organizationId: string | null;
  streamState: StreamState;
  drawerOpen: boolean;
  onSelectSearchResult: (result: SearchResult) => void;
  onToggleDrawer: () => void;
  onSignOut: () => void;
}

export function ConsoleTopbar({
  api,
  organizationId,
  streamState,
  drawerOpen,
  onSelectSearchResult,
  onToggleDrawer,
  onSignOut,
}: ConsoleTopbarProps) {
  const { label, t } = useI18n();
  return (
    <header className='workspace-header topbar'>
      <div className='brand-lockup compact' data-size='sm' data-workspace-leading>
        <span className='brand-mark' data-brand-mark aria-hidden='true'>
          A3
        </span>
        <div data-brand-identity>
          <strong data-brand-name>A3S OS</strong>
          <span data-brand-description>{t('Control plane')}</span>
        </div>
      </div>
      <ResourceSearch
        key={organizationId ?? 'no-organization'}
        api={api}
        organizationId={organizationId}
        onSelect={onSelectSearchResult}
      />
      <div className='topbar-actions' data-workspace-actions>
        <LanguageSwitcher compact />
        <span
          className='status-badge connection-pill'
          data-state={statusBadgeState(streamState)}
          data-size='sm'
          data-indicator
        >
          {label(streamState)}
        </span>
        <button
          className='btn icon-button'
          data-size='icon-sm'
          data-variant='ghost'
          type='button'
          aria-pressed={drawerOpen}
          onClick={onToggleDrawer}
        >
          {drawerOpen ? <PanelRightClose size={18} /> : <PanelRightOpen size={18} />}
          <span className='sr-only'>{drawerOpen ? t('Close operations') : t('Open operations')}</span>
        </button>
        <button
          className='btn quiet-button'
          data-size='sm'
          data-variant='ghost'
          type='button'
          onClick={onSignOut}
        >
          <LogOut size={16} /> {t('Sign out')}
        </button>
      </div>
    </header>
  );
}
