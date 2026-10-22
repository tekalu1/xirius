#!/usr/bin/env python3
"""The app's update key: sign and check the release manifest (UI-282).

The signature is ed25519, written here in the standard library only (the reference
implementation of RFC 8032: SHA-512, the twisted Edwards curve and the prime-order subgroup
check). The signing job runs this file from the tag's checkout, so no build of Rust code touches
the key.

  python scripts/update_sign.py keygen --out <key file>
  python scripts/update_sign.py pubkey --key <key file>
  python scripts/update_sign.py sign --key <key file> --manifest <file> --out <sig> \
      --setup <setup.exe> --version <version>
  python scripts/update_sign.py verify --app <update.rs> --manifest <file> --sig <file> \
      --setup <setup.exe> [--version <version>]

A key file is 64 hex characters (the 32-byte seed); a signature file is 128 hex characters.
The manifest is the JSON `{"version", "file", "sha256", "channel", "min_version"}` and its
signature covers the exact bytes of the file. The steps are in `docs/update-signing.md`.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys

# --- ed25519, the reference implementation of RFC 8032 -------------------------------------

P = 2**255 - 19
Q = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
I = pow(2, (P - 1) // 4, P)


def _h(data: bytes) -> bytes:
    return hashlib.sha512(data).digest()


def _inv(x: int) -> int:
    return pow(x, P - 2, P)


def _xrecover(y: int) -> int:
    xx = (y * y - 1) * _inv(D * y * y + 1)
    x = pow(xx, (P + 3) // 8, P)
    if (x * x - xx) % P != 0:
        x = (x * I) % P
    if x % 2 != 0:
        x = P - x
    return x


BY = 4 * _inv(5) % P
BX = _xrecover(BY)
B = (BX % P, BY % P, 1, (BX * BY) % P)


def _add(a, b):
    x1, y1, z1, t1 = a
    x2, y2, z2, t2 = b
    aa = (y1 - x1) * (y2 - x2) % P
    bb = (y1 + x1) * (y2 + x2) % P
    cc = t1 * 2 * D * t2 % P
    dd = z1 * 2 * z2 % P
    e, f, g, h = bb - aa, dd - cc, dd + cc, bb + aa
    return (e * f % P, g * h % P, f * g % P, e * h % P)


def _mul(s: int, point):
    out = (0, 1, 1, 0)
    while s > 0:
        if s & 1:
            out = _add(out, point)
        point = _add(point, point)
        s >>= 1
    return out


def _compress(point) -> bytes:
    zinv = _inv(point[2])
    x = point[0] * zinv % P
    y = point[1] * zinv % P
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def _decompress(data: bytes):
    if len(data) != 32:
        raise ValueError("a point is 32 bytes")
    y = int.from_bytes(data, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    x = _xrecover(y)
    if (x & 1) != sign:
        x = P - x
    point = (x, y, 1, x * y % P)
    if not _equal(_mul(Q, point), (0, 1, 1, 0)):
        raise ValueError("not in the prime-order subgroup")
    return point


def _equal(a, b) -> bool:
    return (a[0] * b[2] - b[0] * a[2]) % P == 0 and (a[1] * b[2] - b[1] * a[2]) % P == 0


def _expand(secret: bytes):
    if len(secret) != 32:
        raise ValueError("a key is 32 bytes")
    hashed = _h(secret)
    a = int.from_bytes(hashed[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    return a, hashed[32:]


def public_key(secret: bytes) -> bytes:
    a, _ = _expand(secret)
    return _compress(_mul(a, B))


def sign(secret: bytes, message: bytes) -> bytes:
    a, prefix = _expand(secret)
    public = _compress(_mul(a, B))
    r = int.from_bytes(_h(prefix + message), "little") % Q
    r_encoded = _compress(_mul(r, B))
    h = int.from_bytes(_h(r_encoded + public + message), "little") % Q
    s = (r + h * a) % Q
    return r_encoded + int.to_bytes(s, 32, "little")


def verify(public: bytes, message: bytes, signature: bytes) -> bool:
    if len(public) != 32 or len(signature) != 64:
        return False
    try:
        a = _decompress(public)
        r = _decompress(signature[:32])
        s = int.from_bytes(signature[32:], "little")
        if s >= Q:
            return False
        h = int.from_bytes(_h(signature[:32] + public + message), "little") % Q
        return _equal(_mul(s, B), _add(r, _mul(h, a)))
    except ValueError:
        return False


# --- the key and the manifest ---------------------------------------------------------------


SEMVER = re.compile(
    r"^(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*))*"
    r"(-(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
    r"(\.(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*)?"
    r"(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$"
)
KEYS = {"version", "file", "sha256", "channel", "min_version"}
PLACEHOLDER = "0" * 64
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)


def read_key(path: str) -> bytes:
    with open(path, encoding="utf-8") as f:
        text = f.read().strip()
    if len(text) != 64 or not all(c in "0123456789abcdefABCDEF" for c in text):
        raise SystemExit(f"update_sign: {path} is not 64 hex characters")
    return bytes.fromhex(text)


def read_signature(path: str) -> bytes:
    with open(path, encoding="utf-8") as f:
        text = f.read().strip()
    if len(text) != 128 or not all(c in "0123456789abcdefABCDEF" for c in text):
        raise SystemExit(f"update_sign: {path} is not 128 hex characters")
    return bytes.fromhex(text)


def channel_of(version: str) -> str:
    core = version.split("+", 1)[0]
    return "beta" if "-" in core else "stable"


def read_manifest(path: str, setup: str | None, version: str | None) -> tuple[bytes, dict]:
    """The manifest's exact bytes and what it says, checked strictly before it is signed."""
    with open(path, "rb") as f:
        raw = f.read()
    try:
        manifest = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as e:
        raise SystemExit(f"update_sign: {path} is not JSON: {e}")
    if not isinstance(manifest, dict) or set(manifest) != KEYS:
        raise SystemExit(
            f"update_sign: the manifest must hold exactly {sorted(KEYS)}"
        )
    for key in KEYS:
        if not isinstance(manifest[key], str):
            raise SystemExit(f"update_sign: {key} is not a string")
    found = manifest["version"]
    if not SEMVER.match(found):
        raise SystemExit(f"update_sign: {found} is not a semver version")
    if version is not None:
        version = version[1:] if version[:1] in ("v", "V") else version
        if found != version:
            raise SystemExit(
                f"update_sign: the manifest's version {found} is not the tag's {version}"
            )
    if manifest["file"] != f"Xirius-{found}-win-x64-setup.exe":
        raise SystemExit(
            f"update_sign: the file {manifest['file']} is not Xirius-{found}-win-x64-setup.exe"
        )
    if manifest["channel"] != channel_of(found):
        raise SystemExit(
            f"update_sign: the channel {manifest['channel']} is not {channel_of(found)}"
        )
    if not SEMVER.match(manifest["min_version"]):
        raise SystemExit(
            f"update_sign: the minimum version {manifest['min_version']} is not semver"
        )
    if "sha256" not in manifest or len(manifest["sha256"]) != 64:
        raise SystemExit("update_sign: the sha256 is not 64 characters")
    if setup is not None:
        with open(setup, "rb") as f:
            digest = hashlib.sha256(f.read()).hexdigest()
        if manifest["sha256"].lower() != digest:
            raise SystemExit(
                f"update_sign: the sha256 does not match {setup}"
            )
    return raw, manifest


def write_signature(path: str, signature: bytes) -> None:
    with open(path, "w", encoding="ascii", newline="\n") as f:
        f.write(signature.hex() + "\n")


def app_key(path: str) -> bytes:
    with open(path, encoding="utf-8") as f:
        text = f.read()
    match = re.search(r'pub const UPDATE_KEY: &str = "([0-9a-fA-F]{64})";', text)
    if not match:
        raise SystemExit(f"update_sign: no UPDATE_KEY in {path}")
    hex_key = match.group(1)
    if hex_key.lower() == PLACEHOLDER:
        raise SystemExit(
            "update_sign: the app's UPDATE_KEY is still the placeholder (put the public key in)"
        )
    public = bytes.fromhex(hex_key)
    if not verify_public_key(public):
        raise SystemExit("update_sign: UPDATE_KEY is not a public key")
    return public


def verify_public_key(public: bytes) -> bool:
    try:
        _decompress(public)
        return True
    except ValueError:
        return False


# --- the commands ---------------------------------------------------------------------------


def command_keygen(args) -> None:
    if os.path.exists(args.out):
        raise SystemExit(f"update_sign: {args.out} is already there (not overwritten)")
    if inside_repository(args.out):
        raise SystemExit(
            f"update_sign: {args.out} is inside the repository (write the key outside it)"
        )
    secret = os.urandom(32)
    with open(args.out, "x", encoding="ascii", newline="\n") as f:
        f.write(secret.hex() + "\n")
    print(f"secret: {args.out}")
    print(f"public: {public_key(secret).hex()}")
    print("Put the public key in UPDATE_KEY in crates/xirius-session/src/update.rs.")


def inside_repository(path: str) -> bool:
    start = os.path.dirname(os.path.abspath(path))
    while True:
        if os.path.exists(os.path.join(start, ".git")):
            return True
        parent = os.path.dirname(start)
        if parent == start:
            return False
        start = parent


def command_pubkey(args) -> None:
    print(public_key(read_key(args.key)).hex())


def command_sign(args) -> None:
    secret = read_key(args.key)
    raw, _ = read_manifest(args.manifest, args.setup, args.version)
    write_signature(args.out, sign(secret, raw))
    print(f"signature: {args.out}")


def command_verify(args) -> None:
    public = app_key(args.app)
    with open(args.manifest, "rb") as f:
        raw = f.read()
    signature = read_signature(args.sig)
    if not verify(public, raw, signature):
        raise SystemExit("update_sign: the signature does not match")
    _, manifest = read_manifest(args.manifest, args.setup, args.version)
    print(f"ok: {manifest['version']} {manifest['file']} (public {public.hex()})")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description="Sign and check the Xirius update manifest")
    sub = parser.add_subparsers(dest="command", required=True)

    keygen = sub.add_parser("keygen")
    keygen.add_argument("--out", required=True)

    pubkey = sub.add_parser("pubkey")
    pubkey.add_argument("--key", required=True)

    sign_cmd = sub.add_parser("sign")
    sign_cmd.add_argument("--key", required=True)
    sign_cmd.add_argument("--manifest", required=True)
    sign_cmd.add_argument("--out", required=True)
    sign_cmd.add_argument("--setup", required=True)
    sign_cmd.add_argument("--version")

    check = sub.add_parser("verify")
    check.add_argument("--app", required=True)
    check.add_argument("--manifest", required=True)
    check.add_argument("--sig", required=True)
    check.add_argument("--setup", required=True)
    check.add_argument("--version")

    args = parser.parse_args(argv)
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    if hasattr(sys.stderr, "reconfigure"):
        sys.stderr.reconfigure(encoding="utf-8")
    {
        "keygen": command_keygen,
        "pubkey": command_pubkey,
        "sign": command_sign,
        "verify": command_verify,
    }[args.command](args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
