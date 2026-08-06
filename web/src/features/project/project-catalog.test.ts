import { describe, expect, it } from 'vitest';
import {
  ALL_CAPABILITY_GATES,
  CAPABILITY_COUNTS,
  CAPABILITY_GROUPS,
  DOCUMENTATION_VERSIONS,
  PRODUCT_PILLARS,
} from './project-catalog';

describe('project capability catalog', () => {
  it('publishes every authoritative roadmap gate exactly once', () => {
    const codes = ALL_CAPABILITY_GATES.map((capability) => capability.code);

    expect(codes).toHaveLength(19);
    expect(new Set(codes).size).toBe(19);
    expect([...codes].sort()).toEqual(
      [
        'A0',
        'A1',
        'BX0',
        'C0',
        'D0',
        'E0',
        'EV0',
        'F0',
        'G0',
        'H0',
        'I0',
        'MCP0',
        'N0',
        'P0',
        'PW0',
        'R0',
        'S0',
        'U0',
        'W0',
      ].sort()
    );
    expect(CAPABILITY_GROUPS.flatMap((group) => group.gates)).toEqual(ALL_CAPABILITY_GATES);
  });

  it('keeps the roadmap snapshot counts and unavailable gates explicit', () => {
    expect(CAPABILITY_COUNTS).toEqual({
      verified: 1,
      'in-progress': 8,
      recertification: 4,
      planned: 6,
    });
    expect(
      ALL_CAPABILITY_GATES.filter((capability) => capability.unavailable).map((capability) => capability.code)
    ).toEqual(['MCP0', 'U0']);
    expect(ALL_CAPABILITY_GATES.every((capability) => capability.features.length >= 3)).toBe(true);
  });

  it('offers real development and compatibility documentation lines', () => {
    expect(DOCUMENTATION_VERSIONS.map((version) => version.id)).toEqual(['main', '0.1']);
    expect(DOCUMENTATION_VERSIONS[1]?.description.en).toContain('REST v1 contract 1.6.0');
  });

  it('positions the three outward-facing products above the shared runtime foundation', () => {
    expect(PRODUCT_PILLARS.map((pillar) => pillar.id)).toEqual([
      'unified-gateway',
      'workflow',
      'agent-factory',
    ]);
    expect(PRODUCT_PILLARS.map((pillar) => pillar.basedOn)).toEqual([
      'Cloud API + A3S Gateway',
      'A3S Workflow',
      'A3S Runtime + A3S Box',
    ]);
  });
});
