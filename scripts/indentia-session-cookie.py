#!/usr/bin/env python3
"""Print the Indentia using-zone session cookie (indentia_session_0).

Reads Chromium-family cookie DBs (Edge first, then Chrome/Comet) and decrypts
the macOS Safe Storage value. Used by cortex-mem-hook when talking to
https://<tenant>.using.indentia.ai/cortex through Kong.
"""
from __future__ import annotations

import hashlib
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

BROWSERS = (
    (
        "Microsoft Edge Safe Storage",
        "Microsoft Edge",
        Path.home() / "Library/Application Support/Microsoft Edge",
    ),
    (
        "Chrome Safe Storage",
        "Chrome",
        Path.home() / "Library/Application Support/Google/Chrome",
    ),
    (
        "Comet Safe Storage",
        "Comet",
        Path.home() / "Library/Application Support/Comet",
    ),
)


def safe_storage_key(service: str, account: str) -> bytes | None:
    try:
        pw = subprocess.check_output(
            ["security", "find-generic-password", "-w", "-s", service, "-a", account],
            stderr=subprocess.DEVNULL,
        ).decode().strip()
    except subprocess.CalledProcessError:
        return None
    if not pw:
        return None
    return hashlib.pbkdf2_hmac("sha1", pw.encode("utf-8"), b"saltysalt", 1003, dklen=16)


def decrypt(blob: bytes, key: bytes) -> str:
    if not blob:
        return ""
    if blob[:3] in (b"v10", b"v11"):
        cipher = Cipher(algorithms.AES(key), modes.CBC(b" " * 16))
        decryptor = cipher.decryptor()
        out = decryptor.update(blob[3:]) + decryptor.finalize()
        pad = out[-1]
        if 1 <= pad <= 16:
            out = out[:-pad]
        if len(out) > 32:
            out = out[32:]
        return out.decode("utf-8", "replace")
    try:
        return blob.decode("utf-8")
    except UnicodeDecodeError:
        return ""


def extract_from(root: Path, key: bytes) -> str | None:
    dbs = list(root.glob("*/Cookies")) + list(root.glob("*/Network/Cookies"))
    for src in dbs:
        tmp = Path(tempfile.mkstemp(suffix=".db")[1])
        try:
            shutil.copy2(src, tmp)
            con = sqlite3.connect(str(tmp))
            rows = con.execute(
                "SELECT encrypted_value FROM cookies "
                "WHERE name = 'indentia_session_0' "
                "AND host_key IN ('.indentia.ai', 'indentia.ai', "
                "'.using.indentia.ai') "
                "ORDER BY expires_utc DESC"
            ).fetchall()
            con.close()
            for (blob,) in rows:
                value = decrypt(blob, key).strip()
                if value:
                    return value
        except sqlite3.Error:
            continue
        finally:
            tmp.unlink(missing_ok=True)
    return None


def main() -> int:
    for service, account, root in BROWSERS:
        if not root.exists():
            continue
        key = safe_storage_key(service, account)
        if key is None:
            continue
        value = extract_from(root, key)
        if value:
            sys.stdout.write(value)
            return 0
    print("indentia_session_0 not found in Edge/Chrome/Comet", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
