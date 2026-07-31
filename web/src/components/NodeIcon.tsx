import type { NodeKind } from '../types';

export function NodeIcon({ kind, size = 18 }: { kind: NodeKind; size?: number }) {
  const common = {
    width: size,
    height: size,
    viewBox: '0 0 24 24',
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: 1.8,
    strokeLinecap: 'round' as const,
    strokeLinejoin: 'round' as const,
    'aria-hidden': true,
  };
  switch (kind) {
    case 'start':
      return (
        <svg {...common}>
          <path d="M8 5l11 7-11 7V5z" />
        </svg>
      );
    case 'llm':
      return (
        <svg {...common}>
          <path d="M12 3l1.5 5.5L19 10l-5.5 1.5L12 17l-1.5-5.5L5 10l5.5-1.5L12 3z" />
          <path d="M18.5 16l.6 2.4 2.4.6-2.4.6-.6 2.4-.6-2.4-2.4-.6 2.4-.6.6-2.4z" />
        </svg>
      );
    case 'agent':
      return (
        <svg {...common}>
          <rect x="5" y="7" width="14" height="11" rx="3" />
          <path d="M9 12h.01M15 12h.01M9 15h6M12 7V4M10 4h4" />
        </svg>
      );
    case 'tool':
      return (
        <svg {...common}>
          <path d="M14 6a4 4 0 01-5 5L4 16l4 4 5-5a4 4 0 005-5l-3 2-3-3 2-3z" />
        </svg>
      );
    case 'router':
      return (
        <svg {...common}>
          <path d="M5 4v5a3 3 0 003 3h8M5 20v-5a3 3 0 013-3M16 8l4 4-4 4" />
        </svg>
      );
    case 'memory':
      return (
        <svg {...common}>
          <ellipse cx="12" cy="6" rx="7" ry="3" />
          <path d="M5 6v6c0 1.7 3.1 3 7 3s7-1.3 7-3V6M5 12v6c0 1.7 3.1 3 7 3s7-1.3 7-3v-6" />
        </svg>
      );
    case 'approval':
      return (
        <svg {...common}>
          <path d="M12 3l8 4v5c0 5-3.4 8-8 9-4.6-1-8-4-8-9V7l8-4z" />
          <path d="M8.5 12l2.2 2.2 4.8-5" />
        </svg>
      );
    case 'output':
      return (
        <svg {...common}>
          <path d="M14 5h5v14h-5M10 8l4 4-4 4M14 12H3" />
        </svg>
      );
    case 'http':
      return (
        <svg {...common}>
          <circle cx="12" cy="12" r="9" />
          <path d="M3 12h18M12 3a15 15 0 010 18M12 3a15 15 0 000 18" />
        </svg>
      );
    default:
      return (
        <svg {...common}>
          <path d="M5 5h14v14H5zM8 9h8M8 13h5" />
        </svg>
      );
  }
}
