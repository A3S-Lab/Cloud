# GA-1 Gateway public traffic evidence (2026-09-17)

**Status:** `A3S_CLOUD_GA1_GATEWAY_PUBLIC_TRAFFIC_PROVEN` (not full GA-1 CERTIFIED).

## Proven

Pin-matched `a3s-gateway` (`e92896769953aee28ef69261f77265e427f9d396`) carried
HTTP public traffic to the recovered published Agent Service `/health/ready` on
Box `8b2804c585d2f31bce06f213990407606a475634` with artifact
`@sha256:34cfe15ffa81bcd2d75f5e9b72ed5f3dd8ac1a6e9dd83e0b62f57ab30e7ecda1`.

```
A3S_CLOUD_GA1_GATEWAY_PUBLIC_TRAFFIC_PROVEN gateway_public=traffic path=/health/ready gateway_revision=e92896769953aee28ef69261f77265e427f9d396 box_revision=8b2804c585d2f31bce06f213990407606a475634 artifact_digest=sha256:34cfe15ffa81bcd2d75f5e9b72ed5f3dd8ac1a6e9dd83e0b62f57ab30e7ecda1 upstream=127.0.0.1:36891 traffic=127.0.0.1:46387 unit_id=workload:01a0afa2-b089-7002-b749-98a2fcdb00f7:revision:01a0afa2-b089-7002-b749-98b45cd9bd0b generation=1
```

Installer path: `DurableGatewaySnapshotInstaller` against pin-matched
`a3s-gateway`, during the A0.4 recovered Running window (live probe hook).

## Not claimed

- `A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_CERTIFIED` — blocked until
  management-plane conversation/execution + durable events against the same
  release are proven in the same LIVE smoke.
- BX0 Python TLS `:18444` is not Gateway LIVE.

## Artifacts

| File | Meaning |
|------|---------|
| `gateway-public-traffic.txt` | PROVEN marker line |
| `a0-4-release-during-ga1.txt` | A0.4 CERTIFIED from the same run |
| `ga1-gateway-live.tail.log` | Tail of harness output |
| `a0-4-image.env` | Exact published Agent image bind |
| `GATEWAY-REVISION` | Pin-matched gateway revision |
