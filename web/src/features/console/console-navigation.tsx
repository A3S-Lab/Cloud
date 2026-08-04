import { Bot, Boxes, Globe2, LayoutDashboard, PackageCheck } from 'lucide-react';
import type { KeyboardEvent } from 'react';
import type { SearchResourceKind } from '../../types/api';

export type ConsoleSection = 'overview' | 'workloads' | 'agents' | 'delivery' | 'edge';

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
] as const satisfies ReadonlyArray<{
  id: ConsoleSection;
  label: string;
  description: string;
  countKey: keyof ConsoleSectionCounts;
  countLabel: string;
  icon: typeof LayoutDashboard;
}>;

export function ConsoleNavigation({ activeSection, counts, onSelect }: ConsoleNavigationProps) {
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
    <nav className='console-navigation' aria-label='Environment sections'>
      <div role='tablist' aria-label='Environment workspace'>
        {SECTIONS.map((section, index) => {
          const Icon = section.icon;
          const active = section.id === activeSection;
          const count = counts[section.countKey];
          return (
            <button
              id={`console-${section.id}-tab`}
              className={active ? 'active' : undefined}
              type='button'
              role='tab'
              aria-label={`${section.label}, ${section.description}, ${count} ${section.countLabel}`}
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
                <strong>{section.label}</strong>
                <small>{section.description}</small>
              </span>
              <em title={`${count} ${section.countLabel}`}>{count}</em>
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
