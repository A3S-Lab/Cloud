# BX0.software EXIT evidence (`2026-09-17`)

Operator-retained LIVE certification for GA-0 / `BX0.software`.

## Certification line

```text
A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED
  cloud_revision=140a62d04f1309d549f8764426328fc92bbbf473
  runtime_revision=4c5fbd56bedd84d1007a7d9cd046a9f7083bbdcd
  box_revision=8b2804c585d2f31bce06f213990407606a475634
  gateway_revision=e92896769953aee28ef69261f77265e427f9d396
  power_revision=UNBOUND
  profile=software
  loop=included
  receipts=included
```

## Honesty

- Host: Docker-free Linux (`/var/run/docker.sock` absent).
- Path: `run_bx0_software_exit_live_chain.sh` → CREATE enroll through cleanup →
  clean-host gate + collector + exit-audit.
- No invented Power pin; no TEE claim; no docker.sock override theater.

## Bundle layout

| Path | Contents |
| --- | --- |
| `bx0-software-exit-harness.txt` | Product EXIT_CERTIFIED line |
| `software-exit-harness-report.txt` | Harness PASS ledger including GA-0 EXIT |
| `exit-audit/` | Exit audit certification + LOOP cert + profile |
| `loop/` | LOOP certification + checklist |
| `gate/` | Gate / collector / exit-audit stdout |
| `loop-create/` | CREATE enroll→…→cleanup report + scrubbed identities + health/https/logs receipts |
| `RETAINED.txt` | Retention metadata |

Tokens and secrets were not retained.
