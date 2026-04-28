#!/usr/bin/env python3
"""
normalize_url.py — canonicalize a URL for the Tampa-DOGE sitemap.

stdlib only. Reads ONE URL from stdin (or argv[1]), writes the canonical
form to stdout, exits 0. Emits an error to stderr and exits 2 on parse failure.

Canonicalization steps:
  1. Lowercase the scheme and host (URL host is case-insensitive per RFC 3986).
  2. Drop the fragment (#...).
  3. Strip session-token query keys: jsessionid, phpsessid, csrf, csrftoken,
     sessionid, sid, asp.net_sessionid.
  4. Strip tracking query keys: utm_*, fbclid, gclid, mc_cid, mc_eid, ref,
     _ga.
  5. Sort the remaining query keys alphabetically; preserve repeated keys
     in their original relative order.
  6. Strip any path-segment that looks like ;jsessionid=... (matrix-param style).
  7. Collapse duplicate slashes in the path (but keep the leading single /).
  8. Strip trailing slash on the path UNLESS the path is exactly "/".
  9. Default port for scheme is dropped (80 for http, 443 for https).

This is good-enough for v0.1 dedup. Iterate after the first real bootstrap
when we see what tampa.gov actually emits.
"""

from __future__ import annotations

import re
import sys
from urllib.parse import urlsplit, urlunsplit, parse_qsl, urlencode

SESSION_KEYS = {
    "jsessionid",
    "phpsessid",
    "csrf",
    "csrftoken",
    "sessionid",
    "sid",
    "asp.net_sessionid",
}

TRACKING_KEY_PREFIXES = ("utm_",)
TRACKING_KEYS = {"fbclid", "gclid", "mc_cid", "mc_eid", "ref", "_ga"}

DEFAULT_PORTS = {"http": 80, "https": 443}

# Matches ;jsessionid=XXXX or ;PHPSESSID=XXXX inside a path segment.
MATRIX_SESSION_RE = re.compile(
    r";(?:jsessionid|phpsessid|sessionid|sid)=[^/;]*",
    re.IGNORECASE,
)


def _is_session_key(k: str) -> bool:
    kl = k.lower()
    return kl in SESSION_KEYS


def _is_tracking_key(k: str) -> bool:
    kl = k.lower()
    if kl in TRACKING_KEYS:
        return True
    return any(kl.startswith(p) for p in TRACKING_KEY_PREFIXES)


def normalize(url: str) -> str:
    url = url.strip()
    if not url:
        raise ValueError("empty URL")

    parts = urlsplit(url)
    if not parts.scheme or not parts.netloc:
        raise ValueError(f"URL missing scheme or host: {url!r}")

    scheme = parts.scheme.lower()

    # Host: lowercase, strip default port.
    host = parts.hostname or ""
    host = host.lower()
    port = parts.port
    userinfo = ""
    if parts.username:
        userinfo = parts.username
        if parts.password is not None:
            userinfo += ":" + parts.password
        userinfo += "@"
    if port and DEFAULT_PORTS.get(scheme) != port:
        netloc = f"{userinfo}{host}:{port}"
    else:
        netloc = f"{userinfo}{host}"

    # Path: strip matrix session params, collapse //, trim trailing /.
    path = parts.path or "/"
    path = MATRIX_SESSION_RE.sub("", path)
    path = re.sub(r"/{2,}", "/", path)
    if len(path) > 1 and path.endswith("/"):
        path = path.rstrip("/")
    if not path:
        path = "/"

    # Query: drop session + tracking keys, sort the rest stably.
    pairs = parse_qsl(parts.query, keep_blank_values=True)
    kept = [
        (k, v) for (k, v) in pairs
        if not _is_session_key(k) and not _is_tracking_key(k)
    ]
    # Stable sort by key only, so repeated keys keep their original order.
    kept.sort(key=lambda kv: kv[0].lower())
    query = urlencode(kept, doseq=True)

    # Drop fragment.
    fragment = ""

    return urlunsplit((scheme, netloc, path, query, fragment))


def _read_input() -> str:
    if len(sys.argv) > 1:
        return sys.argv[1]
    return sys.stdin.read()


def main() -> int:
    raw = _read_input()
    try:
        out = normalize(raw)
    except ValueError as e:
        print(f"normalize_url: {e}", file=sys.stderr)
        return 2
    print(out)
    return 0


def _selftest() -> None:
    """Run with `python3 normalize_url.py --selftest`."""
    cases = [
        # (input, expected)
        ("HTTPS://Www.Tampa.GOV/Procurement/", "https://www.tampa.gov/Procurement"),
        ("https://www.tampa.gov/page#section", "https://www.tampa.gov/page"),
        (
            "https://www.tampa.gov/page?utm_source=x&id=42&utm_medium=y",
            "https://www.tampa.gov/page?id=42",
        ),
        (
            "https://www.tampa.gov/portal;jsessionid=ABC123/awards",
            "https://www.tampa.gov/portal/awards",
        ),
        (
            "https://www.tampa.gov/p?b=2&a=1",
            "https://www.tampa.gov/p?a=1&b=2",
        ),
        ("http://www.tampa.gov:80/", "http://www.tampa.gov/"),
        ("https://www.tampa.gov:443/x/", "https://www.tampa.gov/x"),
        ("https://www.tampa.gov//foo///bar/", "https://www.tampa.gov/foo/bar"),
        (
            "https://www.tampa.gov/x?JSESSIONID=abc&keep=1",
            "https://www.tampa.gov/x?keep=1",
        ),
    ]
    failed = 0
    for inp, expected in cases:
        got = normalize(inp)
        ok = got == expected
        marker = "OK " if ok else "FAIL"
        print(f"[{marker}] {inp!r}\n        -> {got!r}")
        if not ok:
            print(f"        expected {expected!r}")
            failed += 1
    print()
    if failed:
        print(f"{failed} case(s) failed")
        sys.exit(1)
    print("all selftest cases passed")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--selftest":
        _selftest()
    else:
        sys.exit(main())
