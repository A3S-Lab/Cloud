# Power pin for BX0.5 clean-host product EXIT (PW0)

Product `A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED` requires a bound Power
revision in addition to LOOP certification.

## Pin file

Expected path (override with `A3S_CLOUD_BX0_POWER_REVISION_FILE`):

```text
tools/power-conformance/power-revision
```

Contents: one exact 40-hex Git revision for the Power pin, newline-terminated.

## Rules

- Do **not** invent or placeholder this file to unlock EXIT.
- Absence → exit audit prints `reason=power_unbound` / `EXIT_BLOCKED`.
- Present but not exact lowercase 40-hex → `reason=power_pin_invalid` /
  `EXIT_BLOCKED` (must not collapse into `power_unbound`).
- PW0.1 (immutable Box-hosted Power Service profile) must land before a real
  pin is committed here. See `docs/development-plan.md` milestone PW0.1 and
  A3S-Lab/Power tracking.

Until that pin exists, clean-host gate OPEN lines print
`power_revision=UNBOUND reason=pw0_no_pin_file`.
