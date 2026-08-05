import { ChevronRight } from 'lucide-react';
import { useI18n } from '../../lib/i18n';

interface NamedItem {
  id: string;
  name: string;
}

interface ContextBarProps {
  organizationId: string;
  organizations: NamedItem[];
  organizationLoading: boolean;
  projectId: string;
  projects: NamedItem[];
  environmentId: string;
  environments: NamedItem[];
  onOrganizationChange: (value: string) => void;
  onProjectChange: (value: string) => void;
  onEnvironmentChange: (value: string) => void;
}

export function ContextBar({
  organizationId,
  organizations,
  organizationLoading,
  projectId,
  projects,
  environmentId,
  environments,
  onOrganizationChange,
  onProjectChange,
  onEnvironmentChange,
}: ContextBarProps) {
  const { t } = useI18n();
  return (
    <nav className='context-bar' aria-label={t('Cloud context')}>
      <ContextSelect
        label={t('Organization')}
        value={organizationId}
        items={organizations}
        disabled={organizationLoading}
        onChange={onOrganizationChange}
      />
      <ChevronRight size={15} aria-hidden='true' />
      <ContextSelect label={t('Project')} value={projectId} items={projects} onChange={onProjectChange} />
      <ChevronRight size={15} aria-hidden='true' />
      <ContextSelect
        label={t('Environment')}
        value={environmentId}
        items={environments}
        onChange={onEnvironmentChange}
      />
    </nav>
  );
}

function ContextSelect({
  label,
  value,
  items,
  disabled,
  onChange,
}: {
  label: string;
  value: string;
  items: NamedItem[];
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  const { t } = useI18n();
  return (
    <label className='context-select'>
      <span>{label}</span>
      <select
        value={value}
        disabled={disabled || items.length === 0}
        onChange={(event) => onChange(event.target.value)}
      >
        {items.length === 0 ? <option value=''>{t('None yet')}</option> : null}
        {items.map((item) => (
          <option value={item.id} key={item.id}>
            {item.name}
          </option>
        ))}
      </select>
    </label>
  );
}
