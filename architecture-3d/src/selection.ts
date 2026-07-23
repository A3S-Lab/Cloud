export type ArchitectureSelection =
  | { kind: 'node'; id: string }
  | { kind: 'business-edge'; id: string }
  | { kind: 'structural-edge'; id: string };

export type ArchitectureRelationshipSelection = Exclude<ArchitectureSelection, { kind: 'node' }>;

export function isArchitectureSelection(value: unknown): value is ArchitectureSelection {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as { kind?: unknown; id?: unknown };
  return (
    typeof candidate.id === 'string' &&
    (candidate.kind === 'node' || candidate.kind === 'business-edge' || candidate.kind === 'structural-edge')
  );
}
