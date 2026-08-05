import { LogOut, PanelRightClose, PanelRightOpen, Radio } from 'lucide-react';
import type { CloudApi } from '../../lib/api';
import { LanguageSwitcher, useI18n } from '../../lib/i18n';
import type { SearchResult } from '../../types/api';
import type { StreamState } from '../operations/use-operation-stream';
import { ResourceSearch } from '../search/resource-search';

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
    <header className='topbar'>
      <div className='brand-lockup compact'>
        <span className='brand-mark' aria-hidden='true'>
          A3
        </span>
        <div>
          <strong>A3S OS</strong>
          <span>{t('Control plane')}</span>
        </div>
      </div>
      <ResourceSearch
        key={organizationId ?? 'no-organization'}
        api={api}
        organizationId={organizationId}
        onSelect={onSelectSearchResult}
      />
      <div className='topbar-actions'>
        <LanguageSwitcher compact />
        <span className={`connection-pill ${streamState}`}>
          <Radio size={14} /> {label(streamState)}
        </span>
        <button className='icon-button' type='button' onClick={onToggleDrawer}>
          {drawerOpen ? <PanelRightClose size={18} /> : <PanelRightOpen size={18} />}
          <span className='sr-only'>
            {drawerOpen ? t('Close operations') : t('Open operations')}
          </span>
        </button>
        <button className='quiet-button' type='button' onClick={onSignOut}>
          <LogOut size={16} /> {t('Sign out')}
        </button>
      </div>
    </header>
  );
}
