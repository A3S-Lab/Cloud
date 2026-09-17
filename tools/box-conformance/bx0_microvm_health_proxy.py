#!/usr/bin/env python3
"""Host loopback HTTP proxy for MicroVM boxes (Sandbox port-forward cannot).

For each accepted connection, exec into the guest and fetch 127.0.0.1:GUEST_PORT
via busybox nc, then write the guest bytes back to the host client.
"""
from __future__ import annotations

import os
import socket
import subprocess
import sys
import threading


def handle(client: socket.socket, box_bin: str, box_id: str, guest_port: int, a3s_home: str) -> None:
    try:
        # Drain request (best-effort); guest serves fixed /ready body.
        client.settimeout(2.0)
        try:
            while True:
                chunk = client.recv(4096)
                if not chunk or b"\r\n\r\n" in chunk or b"\n\n" in chunk:
                    break
        except OSError:
            pass
        env = os.environ.copy()
        if a3s_home:
            env["A3S_HOME"] = a3s_home
        env.setdefault("A3S_REGISTRY_PROTOCOL", "http")
        script = (
            "printf 'GET /ready HTTP/1.0\\r\\n\\r\\n' "
            f"| /bin/busybox nc 127.0.0.1 {guest_port}"
        )
        proc = subprocess.run(
            [box_bin, "exec", box_id, "--", "/bin/sh", "-c", script],
            capture_output=True,
            env=env,
            timeout=10,
            check=False,
        )
        if proc.returncode != 0 or not proc.stdout:
            body = (proc.stderr or proc.stdout or b"exec health failed").strip()[:200]
            payload = (
                b"HTTP/1.0 502 Bad Gateway\r\nContent-Type: text/plain\r\n"
                + f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n".encode()
                + body
            )
            client.sendall(payload)
            return
        client.sendall(proc.stdout)
    except Exception as exc:  # noqa: BLE001 — proxy must not die on one client
        try:
            msg = str(exc).encode()[:200]
            client.sendall(
                b"HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\n"
                + f"Content-Length: {len(msg)}\r\nConnection: close\r\n\r\n".encode()
                + msg
            )
        except OSError:
            pass
    finally:
        try:
            client.close()
        except OSError:
            pass


def main() -> int:
    if len(sys.argv) != 6:
        print(
            "usage: bx0_microvm_health_proxy.py BOX_BIN BOX_ID HOST_PORT GUEST_PORT A3S_HOME",
            file=sys.stderr,
        )
        return 2
    box_bin, box_id, host_port_s, guest_port_s, a3s_home = sys.argv[1:]
    host_port = int(host_port_s)
    guest_port = int(guest_port_s)
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind(("127.0.0.1", host_port))
    server.listen(64)
    print(f"microvm-health-proxy listening 127.0.0.1:{host_port} -> {box_id}:{guest_port}", flush=True)
    while True:
        client, _ = server.accept()
        threading.Thread(
            target=handle,
            args=(client, box_bin, box_id, guest_port, a3s_home),
            daemon=True,
        ).start()


if __name__ == "__main__":
    raise SystemExit(main())
