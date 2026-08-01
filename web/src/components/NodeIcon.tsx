import {
  Bot,
  Braces,
  CirclePlay,
  Database,
  GitBranch,
  Globe2,
  LogOut,
  ShieldCheck,
  Sparkles,
  Wrench,
  type LucideIcon,
} from 'lucide-react';
import type { NodeKind } from '../types';

const icons: Record<NodeKind, LucideIcon> = {
  start: CirclePlay,
  template: Braces,
  llm: Sparkles,
  agent: Bot,
  tool: Wrench,
  router: GitBranch,
  memory: Database,
  http: Globe2,
  approval: ShieldCheck,
  output: LogOut,
};

export function NodeIcon({ kind, size = 18 }: { kind: NodeKind; size?: number }) {
  const Glyph = icons[kind];
  return <Glyph aria-hidden="true" size={size} strokeWidth={1.75} />;
}
