#!/usr/bin/env python3
"""Tests of scripts/update_sign.py: python scripts/test_update_sign.py

The ed25519 part is checked with RFC 8032's vectors (7.1, tests 1 to 3). The cross-language
vector (seed 07*32 and the manifest below) is the same in the Rust tests, so a signature made
here is accepted by the app's check.
"""

import hashlib
import importlib.util
import json
import sys
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("update_sign", HERE / "update_sign.py")
us = importlib.util.module_from_spec(spec)
sys.modules["update_sign"] = us
spec.loader.exec_module(us)

# RFC 8032, 7.1: secret, public, message, signature.
RFC8032 = [
    (
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        "",
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155"
        "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    ),
    (
        "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        "72",
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da"
        "085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    ),
    (
        "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
        "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        "af82",
        "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac"
        "18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
    ),
]

# The same seed and manifest as the Rust test `a_python_signed_manifest_is_accepted`.
SEED = bytes([7]) * 32
PUBLIC = "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c"
BODY = b"the installer"
SHA = "36b88446586b04e3c86320db363516229ae74a07ec30928b800aee7d751374da"
MANIFEST = (
    '{"channel":"stable","file":"Xirius-1.0.1-win-x64-setup.exe",'
    '"min_version":"1.0.0","sha256":"' + SHA + '","version":"1.0.1"}\n'
)
SIGNATURE = (
    "36ce109fb240336ce5a5898b3b1dc9a23a5a2e94a69da9fecd7539a479003968"
    "ab163877c160e7c8981b18bba823d64b0c5d3ee96cd9bf43c21f1d6e253c500e"
)


class Ed25519Tests(unittest.TestCase):
    def test_rfc_8032_vectors(self):
        for secret, public, message, signature in RFC8032:
            secret = bytes.fromhex(secret)
            public = bytes.fromhex(public)
            message = bytes.fromhex(message)
            signature = bytes.fromhex(signature)
            self.assertEqual(us.public_key(secret), public)
            self.assertEqual(us.sign(secret, message), signature)
            self.assertTrue(us.verify(public, message, signature))
            self.assertFalse(us.verify(public, message + b"x", signature))

    def test_the_key_and_the_manifest_are_the_ones_the_rust_side_checks(self):
        self.assertEqual(us.public_key(SEED).hex(), PUBLIC)
        raw = MANIFEST.encode("utf-8")
        self.assertEqual(us.sign(SEED, raw).hex(), SIGNATURE)
        self.assertTrue(us.verify(bytes.fromhex(PUBLIC), raw, bytes.fromhex(SIGNATURE)))

    def test_a_bad_signature_is_refused(self):
        signature = bytearray(bytes.fromhex(SIGNATURE))
        signature[0] ^= 1
        self.assertFalse(us.verify(bytes.fromhex(PUBLIC), MANIFEST.encode(), bytes(signature)))
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bad.sig"
            path.write_text("00", encoding="utf-8")
            with self.assertRaises(SystemExit):
                us.read_signature(str(path))


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.setup = self.dir / "Xirius-1.0.1-win-x64-setup.exe"
        self.setup.write_bytes(BODY)
        self.manifest = self.dir / "Xirius-1.0.1-win-x64-update.json"
        self.manifest.write_text(MANIFEST, encoding="utf-8", newline="\n")

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, name, text):
        path = self.dir / name
        path.write_text(text, encoding="utf-8", newline="\n")
        return str(path)

    def check(self, manifest=None, setup=None, version="1.0.1"):
        path = self.manifest
        if manifest is not None:
            path = Path(self.write("m.json", manifest))
        return us.read_manifest(str(path), str(setup or self.setup), version)

    def test_the_manifest_is_read_when_everything_matches(self):
        raw, manifest = self.check()
        self.assertEqual(raw, MANIFEST.encode())
        self.assertEqual(manifest["version"], "1.0.1")
        self.assertEqual(manifest["channel"], "stable")
        self.assertEqual(manifest["min_version"], "1.0.0")

    def test_only_the_five_keys_are_taken(self):
        extra = MANIFEST.replace('{"channel"', '{"extra":"x","channel"')
        with self.assertRaises(SystemExit):
            self.check(extra)
        missing = MANIFEST.replace(',"min_version":"1.0.0"', "")
        with self.assertRaises(SystemExit):
            self.check(missing)
        with self.assertRaises(SystemExit):
            self.check("not json")

    def test_the_version_the_name_and_the_channel_must_fit(self):
        with self.assertRaises(SystemExit):
            self.check(version="9.9.9")
        wrong_name = MANIFEST.replace("setup.exe", "setup.zip")
        with self.assertRaises(SystemExit):
            self.check(wrong_name)
        wrong_channel = MANIFEST.replace('"stable"', '"beta"')
        with self.assertRaises(SystemExit):
            self.check(wrong_channel)

    def test_the_sha256_is_the_installers(self):
        other = self.dir / "other.exe"
        other.write_bytes(b"another installer")
        with self.assertRaises(SystemExit):
            self.check(setup=other)

    def test_the_minimum_version_is_semver(self):
        bad = MANIFEST.replace('"min_version":"1.0.0"', '"min_version":"1.0"')
        self.assertTrue(self.check(bad)[1]["min_version"] == "1.0")  # 1.0 is accepted by SEMVER
        worse = MANIFEST.replace('"min_version":"1.0.0"', '"min_version":"latest"')
        with self.assertRaises(SystemExit):
            self.check(worse)


class CommandTests(unittest.TestCase):
    def test_keygen_makes_a_key_the_rust_tool_reads(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "update.key"
            us.command_keygen(Namespace(out=str(path)))
            self.assertEqual(path.read_text().strip(), us.read_key(str(path)).hex())
            with self.assertRaises(SystemExit):
                us.command_keygen(Namespace(out=str(path)))
            # The format is the Rust tool's: 64 hex characters (the 32-byte seed).
            self.assertRegex(path.read_text().strip(), r"^[0-9a-f]{64}$")

    def test_signing_and_the_app_check(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            key = tmp / "update.key"
            key.write_text(SEED.hex() + "\n", encoding="ascii", newline="\n")
            setup = tmp / "Xirius-1.0.1-win-x64-setup.exe"
            setup.write_bytes(BODY)
            manifest = tmp / "Xirius-1.0.1-win-x64-update.json"
            manifest.write_text(MANIFEST, encoding="utf-8", newline="\n")
            sig = tmp / "manifest.sig"
            us.command_sign(
                Namespace(
                    key=str(key),
                    manifest=str(manifest),
                    out=str(sig),
                    setup=str(setup),
                    version="v1.0.1",
                )
            )
            self.assertEqual(sig.read_text().strip(), SIGNATURE)
            app = tmp / "update.rs"
            app.write_text(f'pub const UPDATE_KEY: &str = "{PUBLIC}";\n', encoding="utf-8")
            us.command_verify(
                Namespace(
                    app=str(app),
                    manifest=str(manifest),
                    sig=str(sig),
                    setup=str(setup),
                    version="1.0.1",
                )
            )
            # The placeholder is refused.
            app.write_text(f'pub const UPDATE_KEY: &str = "{"0" * 64}";\n', encoding="utf-8")
            with self.assertRaises(SystemExit):
                us.command_verify(
                    Namespace(
                        app=str(app),
                        manifest=str(manifest),
                        sig=str(sig),
                        setup=str(setup),
                        version=None,
                    )
                )


if __name__ == "__main__":
    sys.exit(unittest.main())
