#!/usr/bin/env python3
"""
check_robots.py — robots.txt allow/deny check for the Centinel Cartographer.

stdlib only (urllib.robotparser). Reads ONE URL from stdin (or argv[1]),
fetches that host's /robots.txt, evaluates against the Centinel
user-agent, and prints exactly one of:

  ALLOW
  DISALLOW

Exit codes:
  0 — robots.txt was reachable and a verdict was reached
  2 — input parse error
  3 — robots.txt unreachable (we treat unreachable as ALLOW per RFC 9309
      "if no robots.txt is present, the crawler may assume there are no
      restrictions" — we still print ALLOW and exit 0, but log the reason
      to stderr; exit 3 is reserved for a hard fetch error you want the
      caller to handle differently if they care)

User-agent: TampaDOGE/0.1 (+contact)
"""

from __future__ import annotations

import sys
import urllib.error
import urllib.request
from urllib.parse import urlsplit, urlunsplit
from urllib.robotparser import RobotFileParser

USER_AGENT = "TampaDOGE/0.1 (+contact)"
TIMEOUT_SECS = 10


def robots_url_for(url: str) -> str:
    parts = urlsplit(url)
    if not parts.scheme or not parts.netloc:
        raise ValueError(f"URL missing scheme or host: {url!r}")
    return urlunsplit((parts.scheme, parts.netloc, "/robots.txt", "", ""))


def fetch_robots(robots_url: str) -> str | None:
    """Return robots.txt text, or None if unreachable / 404."""
    req = urllib.request.Request(
        robots_url,
        headers={"User-Agent": USER_AGENT},
    )
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT_SECS) as resp:
            charset = resp.headers.get_content_charset() or "utf-8"
            return resp.read().decode(charset, errors="replace")
    except urllib.error.HTTPError as e:
        if e.code in (401, 403):
            # Per robotparser convention: 401/403 = disallow all
            return "User-agent: *\nDisallow: /\n"
        if e.code == 404:
            return ""  # no robots.txt = allow all
        print(f"check_robots: HTTPError {e.code} on {robots_url}", file=sys.stderr)
        return None
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        print(f"check_robots: fetch failed {robots_url}: {e}", file=sys.stderr)
        return None


def is_allowed(url: str) -> bool:
    robots_url = robots_url_for(url)
    text = fetch_robots(robots_url)
    if text is None:
        # Unreachable. Default to ALLOW per RFC 9309 — caller can override
        # by checking exit code / stderr if they want stricter behavior.
        print(
            f"check_robots: robots.txt unreachable at {robots_url}; "
            "defaulting to ALLOW",
            file=sys.stderr,
        )
        return True
    rp = RobotFileParser()
    rp.parse(text.splitlines())
    return rp.can_fetch(USER_AGENT, url)


def _read_input() -> str:
    if len(sys.argv) > 1:
        return sys.argv[1]
    return sys.stdin.read().strip()


def main() -> int:
    raw = _read_input()
    if not raw:
        print("check_robots: empty input", file=sys.stderr)
        return 2
    try:
        verdict = is_allowed(raw)
    except ValueError as e:
        print(f"check_robots: {e}", file=sys.stderr)
        return 2
    print("ALLOW" if verdict else "DISALLOW")
    return 0


if __name__ == "__main__":
    sys.exit(main())
