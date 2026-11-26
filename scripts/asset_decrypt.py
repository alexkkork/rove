#!/usr/bin/env python3
"""Probe/decrypt tool for Roblox encrypted asset payloads.

Mirrors the Rust `asset_crypto` detection exactly so a payload can be checked
outside a live session: same key decoding, same nonce layouts, same "the GCM
tag decides" rule.

  ./asset_decrypt.py --selftest
  ./asset_decrypt.py --key <b64|hex> --in payload.bin
  ./asset_decrypt.py --key-file key.txt --in payload.bin --out plain.bin
  ./asset_decrypt.py --seed-blob seed.b64 --in payload.bin     # 64-byte seed
  ./asset_decrypt.py --key <...> --in payload.bin --scan       # try layouts
  ./asset_decrypt.py --from-join join.log --in payload.bin     # seed from ROVE_DUMP_JOIN

Grab a log with ROVE_DUMP_JOIN=1 and pipe it here; both RandomSeed1 and
SessionId.RandomSeed2 are picked up automatically.

Exit codes: 0 decrypted, 2 tag mismatch, 3 malformed, 4 bad key, 5 usage.
"""
import argparse
import base64
import binascii
import sys

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

TAG = 16
NONCE = 12


def decode_key(text):
    """Base64 (standard or url-safe, padded or not) or hex -> key bytes.

    64-byte seed blobs keep their leading 32 bytes: that is the AES key and the
    tail is padding/nonce material.
    """
    text = text.strip()
    if not text:
        raise ValueError("empty key")
    if len(text) % 2 == 0 and all(c in "0123456789abcdefABCDEF" for c in text):
        raw = bytes.fromhex(text)
    else:
        cleaned = text.replace("-", "+").replace("_", "/").rstrip("=")
        cleaned += "=" * (-len(cleaned) % 4)
        try:
            raw = base64.b64decode(cleaned, validate=True)
        except (binascii.Error, ValueError) as exc:
            raise ValueError("key is not base64 or hex") from exc
    if len(raw) == 64:
        raw = raw[:32]
    if len(raw) not in (16, 24, 32):
        raise ValueError(f"key is {len(raw)} bytes, need 16, 24 or 32")
    return raw


def open_gcm(key, nonce, body):
    return AESGCM(key).decrypt(nonce, body, None)


def layouts(payload):
    """Candidate (name, nonce, body) tuples, in the same order as the Rust side."""
    if len(payload) > TAG + NONCE:
        yield "nonce-prefix-12", payload[:NONCE], payload[NONCE:]
    if len(payload) > TAG + NONCE:
        yield "nonce-suffix-12", payload[-NONCE:], payload[:-NONCE]
    if len(payload) > TAG + 16:
        yield "iv16-prefix", payload[:NONCE], payload[16:]
    if len(payload) >= TAG + 1:
        yield "zero-nonce", b"\x00" * NONCE, payload


def decrypt(key, payload, scan=False):
    """Return (plaintext, layout). Raises on failure."""
    if len(payload) < TAG + 1:
        raise ValueError("payload too short to be GCM")
    errors = []
    for name, nonce, body in layouts(payload):
        try:
            plain = open_gcm(key, nonce, body)
        except Exception as exc:  # InvalidTag and friends
            errors.append(f"{name}: {type(exc).__name__}")
            if scan:
                print(f"  {name}: reject", file=sys.stderr)
            continue
        if scan:
            print(f"  {name}: OK", file=sys.stderr)
        return plain, name
    raise InvalidTag(errors)


class InvalidTag(Exception):
    pass


def scrape_join_seeds(path):
    """Collect base64 seeds from a ROVE_DUMP_JOIN log (RandomSeed1 + nested Seed2)."""
    import json
    import re

    seeds = []
    with open(path, errors="replace") as fh:
        for line in fh:
            if "JOIN_FIELD RandomSeed1 " in line:
                # ... len=N "<value>"
                value = line.split("JOIN_FIELD RandomSeed1 ", 1)[1]
                value = value.split(" ", 1)[1].strip().strip('"')
                if value:
                    seeds.append(value)
            elif "JOIN_FIELD SessionId " in line:
                raw = line.split("JOIN_FIELD SessionId ", 1)[1].split(" ", 1)[1].strip()
                try:
                    parsed = json.loads(raw.strip('"').replace('\\"', '"'))
                except (ValueError, json.JSONDecodeError):
                    continue
                nested = parsed.get("RandomSeed2")
                if nested:
                    seeds.append(nested)
    return seeds


def selftest():
    import os

    ok = True
    key = os.urandom(32)
    body = b"crossroads texture bytes" * 8

    for layout in ("prefix", "suffix", "iv16", "zero"):
        nonce = os.urandom(NONCE) if layout != "zero" else b"\x00" * NONCE
        sealed = AESGCM(key).encrypt(nonce, body, None)
        if layout == "prefix":
            payload = nonce + sealed
        elif layout == "suffix":
            payload = sealed + nonce
        elif layout == "iv16":
            payload = nonce + b"\x00\x00\x00\x01" + sealed
        else:
            payload = sealed
        try:
            plain, used = decrypt(key, payload)
        except Exception as exc:
            print(f"FAIL {layout}: {exc}")
            ok = False
            continue
        if plain != body:
            print(f"FAIL {layout}: plaintext differs")
            ok = False
            continue
        print(f"ok   {layout:8s} -> {used}")

    # Wrong key must fail as a tag mismatch, not return garbage.
    sealed = AESGCM(key).encrypt(os.urandom(NONCE), body, None)
    try:
        decrypt(os.urandom(32), os.urandom(NONCE) + sealed)
        print("FAIL wrong-key: decrypted")
        ok = False
    except InvalidTag:
        print("ok   wrong-key -> tag mismatch")

    # Key decoding shapes.
    for text, expect in (
        ("ab" * 32, 32),
        (base64.b64encode(b"\x01" * 16).decode(), 16),
        (base64.b64encode(b"\x02" * 64).decode(), 32),  # seed blob trims to 32
    ):
        got = len(decode_key(text))
        status = "ok  " if got == expect else "FAIL"
        ok &= got == expect
        print(f"{status} key {len(text):3d} chars -> {got} bytes (want {expect})")

    # "zz"*16 is valid base64 for 24 bytes (AES-192), so it is not a bad key.
    for bad in ("", "short", "ab" * 17, "**" * 16, "not base64 at all!"):
        # 17 bytes of hex, invalid base64 chars, and prose with spaces.

        try:
            decode_key(bad)
            print(f"FAIL key {bad[:12]!r} accepted")
            ok = False
        except ValueError:
            print(f"ok   key {bad[:12]!r} rejected")

    print("\nSELFTEST", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--key")
    ap.add_argument("--key-file")
    ap.add_argument("--seed-blob", help="base64 64-byte seed; leading 32 bytes are the key")
    ap.add_argument("--from-join", help="ROVE_DUMP_JOIN log to scrape seeds from")
    ap.add_argument("--in", dest="input", help="payload file")
    ap.add_argument("--out", dest="output")
    ap.add_argument("--scan", action="store_true", help="report every layout attempt")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    raw_key = args.key
    if args.key_file:
        with open(args.key_file) as fh:
            raw_key = fh.read()
    if args.seed_blob:
        raw_key = args.seed_blob
    if args.from_join:
        seeds = scrape_join_seeds(args.from_join)
        if not seeds:
            print(f"no RandomSeed1/RandomSeed2 in {args.from_join}", file=sys.stderr)
            return 4
        print(f"found {len(seeds)} seed(s) in join dump", file=sys.stderr)
        raw_key = seeds[0]
    if not raw_key or not args.input:
        ap.print_help()
        return 5

    try:
        key = decode_key(raw_key)
    except ValueError as exc:
        print(f"bad key: {exc}", file=sys.stderr)
        return 4

    payload = open(args.input, "rb").read() if args.input != "-" else sys.stdin.buffer.read()
    print(f"key {len(key)}B, payload {len(payload)}B", file=sys.stderr)

    try:
        plain, used = decrypt(key, payload, scan=args.scan)
    except ValueError as exc:
        print(f"malformed: {exc}", file=sys.stderr)
        return 3
    except InvalidTag as exc:
        print(f"tag mismatch, tried: {exc}", file=sys.stderr)
        return 2

    print(f"decrypted via {used}, {len(plain)}B", file=sys.stderr)
    if args.output:
        open(args.output, "wb").write(plain)
        print(f"wrote {args.output}", file=sys.stderr)
    else:
        sys.stdout.buffer.write(plain)
    return 0


if __name__ == "__main__":
    sys.exit(main())
