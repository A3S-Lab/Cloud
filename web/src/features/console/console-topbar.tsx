import { LogOut, PanelRightClose, PanelRightOpen, Radio } from 'lucide-react';
import type { CloudApi } from '../../lib/api';
import type { SearchResult } from '../../types/api';
import type { StreamState } from '../operations/use-operation-stream';
import { ResourceSearch } from '../search/resource-search';
import { streamLabel } from './console-format';

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
  return (
    <header className='topbar'>
      <div className='brand-lockup compact'>
        <span className='brand-mark' aria-hidden='true'>
          A3
        </span>
        <div>
          <strong>A3S Cloud</strong>
          <span>Control plane</span>
        </div>
      </div>
      <ResourceSearch
        key={organizationId ?? 'no-organization'}
        api={api}
        organizationId={organizationId}
        onSelect={onSelectSearchResult}
      />
      <div className='topbar-actions'>
        <span className={`connection-pill ${streamState}`}>
          <Radio size={14} /> {streamLabel(streamState)}
        </span>
        <button className='icon-button' type='button' onClick={onToggleDrawer}>
          {drawerOpen ? <PanelRightClose size={18} /> : <PanelRightOpen size={18} />}
          <span className='sr-only'>{drawerOpen ? 'Close operations' : 'Open operations'}</span>
        </button>
        <button className='quiet-button' type='button' onClick={onSignOut}>
          <LogOut size={16} /> Sign out
        </button>
      </div>
    </header>
  );
}
