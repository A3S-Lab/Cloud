import { ChevronRight } from 'lucide-react';

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
  return (
    <nav className='context-bar' aria-label='Cloud context'>
      <ContextSelect
        label='Organization'
        value={organizationId}
        items={organizations}
        disabled={organizationLoading}
        onChange={onOrganizationChange}
      />
      <ChevronRight size={15} aria-hidden='true' />
      <ContextSelect label='Project' value={projectId} items={projects} onChange={onProjectChange} />
      <ChevronRight size={15} aria-hidden='true' />
      <ContextSelect
        label='Environment'
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
  return (
    <label className='context-select'>
      <span>{label}</span>
      <select
        value={value}
        disabled={disabled || items.length === 0}
        onChange={(event) => onChange(event.target.value)}
      >
        {items.length === 0 ? <option value=''>None yet</option> : null}
        {items.map((item) => (
          <option value={item.id} key={item.id}>
            {item.name}
          </option>
        ))}
      </select>
    </label>
  );
}
