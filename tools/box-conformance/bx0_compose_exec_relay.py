#!/usr/bin/env python3
"""Host TCP relay into an a3s-box compose guest via `compose exec -i` + guest nc.

Used when passt --tcp-ports dies after virtio-net attach (host port_map silent)
but the guest workload is still healthy. Real data path through a3s-box — not a fake pin.
"""
from __future__ import annotations

import argparse
import os
import socket
import subprocess
import sys
import threading


def copy_sock_to_proc(sock: socket.socket, proc: subprocess.Popen[bytes]) -> None:
    assert proc.stdin is not None
    try:
        while True:
            data = sock.recv(65536)
            if not data:
                break
            proc.stdin.write(data)
            proc.stdin.flush()
    except (BrokenPipeError, ConnectionResetError, OSError):
        pass
    finally:
        try:
            proc.stdin.close()
        except OSError:
            pass


def copy_proc_to_sock(sock: socket.socket, proc: subprocess.Popen[bytes]) -> None:
    assert proc.stdout is not None
    try:
        while True:
            data = proc.stdout.read(65536)
            if not data:
                break
            sock.sendall(data)
    except (BrokenPipeError, ConnectionResetError, OSError):
        pass
    finally:
        try:
            sock.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def handle(
    conn: socket.socket,
    box_bin: str,
    compose_acl: str,
    service: str,
    guest_port: int,
) -> None:
    cmd = [
        box_bin,
        "compose",
        "--file",
        compose_acl,
        "exec",
        "--timeout",
        "300",
        "-i",
        service,
        "--",
        "nc",
        "127.0.0.1",
        str(guest_port),
    ]
    proc = subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        bufsize=0,
    )
    t1 = threading.Thread(target=copy_sock_to_proc, args=(conn, proc), daemon=True)
    t2 = threading.Thread(target=copy_proc_to_sock, args=(conn, proc), daemon=True)
    t1.start()
    t2.start()
    t1.join()
    t2.join(timeout=2)
    try:
        proc.kill()
    except OSError:
        pass
    try:
        conn.close()
    except OSError:
        pass


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--box-bin", required=True)
    parser.add_argument("--compose-acl", required=True)
    parser.add_argument("--service", required=True)
    parser.add_argument("--host-port", type=int, required=True)
    parser.add_argument("--guest-port", type=int, required=True)
    parser.add_argument("--bind", default="127.0.0.1")
    args = parser.parse_args()

    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind((args.bind, args.host_port))
    srv.listen(64)
    print(
        f"relay {args.bind}:{args.host_port} -> compose:{args.service}:{args.guest_port}",
        flush=True,
    )
    while True:
        conn, _ = srv.accept()
        threading.Thread(
            target=handle,
            args=(conn, args.box_bin, args.compose_acl, args.service, args.guest_port),
            daemon=True,
        ).start()


if __name__ == "__main__":
    sys.exit(main())
