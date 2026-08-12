import { Bot, Boxes, Globe2, LayoutDashboard, Network, PackageCheck } from 'lucide-react';
import type { KeyboardEvent } from 'react';
import { useI18n } from '../../lib/i18n';
import type { SearchResourceKind } from '../../types/api';

export type ConsoleSection = 'overview' | 'workloads' | 'agents' | 'delivery' | 'edge' | 'architecture';

export interface ConsoleSectionCounts {
  workloads: number;
  agents: number;
  delivery: number;
  edge: number;
  operations: number;
}

interface ConsoleNavigationProps {
  activeSection: ConsoleSection;
  counts: ConsoleSectionCounts;
  onSelect: (section: ConsoleSection) => void;
}

const SECTIONS = [
  {
    id: 'overview',
    label: 'Overview',
    description: 'Workspace health',
    countKey: 'operations',
    countLabel: 'active operations',
    icon: LayoutDashboard,
  },
  {
    id: 'workloads',
    label: 'Workloads',
    description: 'Runtime convergence',
    countKey: 'workloads',
    countLabel: 'workloads',
    icon: Boxes,
  },
  {
    id: 'agents',
    label: 'Agents',
    description: 'Conversations and runs',
    countKey: 'agents',
    countLabel: 'conversations',
    icon: Bot,
  },
  {
    id: 'delivery',
    label: 'Delivery',
    description: 'Builds and evidence',
    countKey: 'delivery',
    countLabel: 'build runs',
    icon: PackageCheck,
  },
  {
    id: 'edge',
    label: 'Edge',
    description: 'Routes and TLS',
    countKey: 'edge',
    countLabel: 'routes',
    icon: Globe2,
  },
  {
    id: 'architecture',
    label: 'Architecture',
    description: 'Platform module map',
    countKey: undefined,
    countLabel: undefined,
    icon: Network,
  },
] as const satisfies ReadonlyArray<{
  id: ConsoleSection;
  label: string;
  description: string;
  countKey?: keyof ConsoleSectionCounts;
  countLabel?: string;
  icon: typeof LayoutDashboard;
}>;

export function ConsoleNavigation({ activeSection, counts, onSelect }: ConsoleNavigationProps) {
  const { t } = useI18n();
  const selectFromKeyboard = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let nextIndex: number | null = null;
    if (event.key === 'ArrowRight') nextIndex = (index + 1) % SECTIONS.length;
    if (event.key === 'ArrowLeft') nextIndex = (index - 1 + SECTIONS.length) % SECTIONS.length;
    if (event.key === 'Home') nextIndex = 0;
    if (event.key === 'End') nextIndex = SECTIONS.length - 1;
    if (nextIndex === null) return;

    event.preventDefault();
    const nextSection = SECTIONS[nextIndex];
    onSelect(nextSection.id);
    event.currentTarget.parentElement
      ?.querySelector<HTMLButtonElement>(`#console-${nextSection.id}-tab`)
      ?.focus();
  };

  return (
    <nav className='tabs console-navigation' aria-label={t('Environment sections')}>
      <div
        role='tablist'
        aria-label={t('Environment workspace')}
        aria-orientation='horizontal'
        data-variant='line'
      >
        {SECTIONS.map((section, index) => {
          const Icon = section.icon;
          const active = section.id === activeSection;
          const count = section.countKey ? counts[section.countKey] : null;
          const ariaLabel =
            count === null
              ? `${t(section.label)}, ${t(section.description)}`
              : `${t(section.label)}, ${t(section.description)}, ${count} ${t(section.countLabel ?? '')}`;
          return (
            <button
              id={`console-${section.id}-tab`}
              className={active ? 'active' : undefined}
              type='button'
              role='tab'
              aria-label={ariaLabel}
              aria-controls={`console-${section.id}-panel`}
              aria-current={active ? 'page' : undefined}
              aria-selected={active}
              tabIndex={active ? 0 : -1}
              onClick={() => onSelect(section.id)}
              onKeyDown={(event) => selectFromKeyboard(event, index)}
              key={section.id}
            >
              <Icon size={17} aria-hidden='true' />
              <span>
                <strong>{t(section.label)}</strong>
                <small>{t(section.description)}</small>
              </span>
              {count === null ? null : (
                <em
                  className='badge'
                  data-variant='secondary'
                  title={`${count} ${t(section.countLabel ?? '')}`}
                >
                  {count}
                </em>
              )}
            </button>
          );
        })}
      </div>
    </nav>
  );
}

export function sectionForResourceKind(kind: SearchResourceKind | null): ConsoleSection {
  if (kind === 'workload' || kind === 'deployment') return 'workloads';
  if (kind === 'build_run' || kind === 'source_revision') return 'delivery';
  if (kind === 'route' || kind === 'domain_claim' || kind === 'gateway_scope') return 'edge';
  return 'overview';
}
