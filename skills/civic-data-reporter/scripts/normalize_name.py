#!/usr/bin/env python3
"""
normalize_name.py — canonicalize a person or org name.

Reads a single name from stdin (or --name=<...>), prints the canonical form
to stdout. Stdlib only. See references/name-normalization.md for the rules.

Usage:
    echo "Smith, John A." | python normalize_name.py --kind=person
    python normalize_name.py --kind=org --name="ACME Construction, LLC"
    python normalize_name.py --selftest
"""
from __future__ import annotations

import argparse
import re
import sys
import unicodedata
from typing import Tuple

HONORIFICS = {
    "mr", "mrs", "ms", "mx", "dr", "hon", "rev", "prof",
    "sir", "dame", "the", "honorable",
}
SUFFIXES = {"jr", "sr", "ii", "iii", "iv", "v", "esq", "phd", "md"}
CORP_SUFFIXES = {
    "llc", "inc", "incorporated", "corp", "corporation",
    "co", "company", "ltd", "limited", "lp", "llp", "pllc",
    "pa", "na",
}
PARTICLES = {"van", "de", "la", "von", "del", "der", "bin", "ibn",
             "da", "das", "do", "dos", "du"}


def _strip_dots(tok: str) -> str:
    return tok.replace(".", "")


def _is_acronym(tok: str) -> bool:
    return 2 <= len(tok) <= 6 and tok.isupper() and tok.isalpha()


def _titlecase_token(tok: str, force_acronym: bool = False) -> str:
    if force_acronym or _is_acronym(tok):
        return tok
    if tok.lower() in PARTICLES:
        return tok.lower()
    # Handle hyphens and apostrophes inside the token.
    parts = re.split(r"([-'])", tok)
    out = []
    for p in parts:
        if p in ("-", "'"):
            out.append(p)
        elif not p:
            out.append(p)
        else:
            out.append(p[:1].upper() + p[1:].lower())
    return "".join(out)


def normalize_person(name: str) -> str:
    s = name.strip().strip("\"'").strip()
    if not s:
        return ""

    # Detect "Last, First M." -> reorder.
    if s.count(",") == 1:
        left, right = [x.strip() for x in s.split(",", 1)]
        # Don't reorder if right looks like a corporate suffix.
        if right and _strip_dots(right).lower().split()[0] not in CORP_SUFFIXES:
            s = f"{right} {left}"

    # Tokenize, drop empties.
    raw_tokens = [_strip_dots(t) for t in re.split(r"\s+", s) if t]
    if not raw_tokens:
        return ""

    # Drop leading honorifics (possibly stacked).
    while raw_tokens and raw_tokens[0].lower() in HONORIFICS:
        raw_tokens.pop(0)

    # Pull off trailing suffix(es).
    trailing_suffix = None
    if raw_tokens and raw_tokens[-1].lower() in SUFFIXES:
        trailing_suffix = raw_tokens.pop().lower()

    if not raw_tokens:
        return ""

    # Collapse a bare middle initial to single letter (no dot).
    out_tokens = []
    for t in raw_tokens:
        if len(t) == 1 and t.isalpha():
            out_tokens.append(t.upper())
        else:
            out_tokens.append(_titlecase_token(t))

    canonical = " ".join(out_tokens)
    if trailing_suffix:
        # Title-case roman numerals as upper, others as Title.
        if trailing_suffix in ("ii", "iii", "iv", "v"):
            canonical = f"{canonical} {trailing_suffix.upper()}"
        else:
            canonical = f"{canonical} {trailing_suffix.capitalize()}"
    # Collapse whitespace.
    canonical = re.sub(r"\s+", " ", canonical).strip()
    return canonical


def normalize_org(name: str) -> str:
    s = name.strip().strip("\"'").strip()
    if not s:
        return ""
    # Drop a leading "The ".
    if s.lower().startswith("the "):
        s = s[4:]

    # Split on dba/aka — keep primary only.
    s = re.split(r"\b(?:dba|d/b/a|a/k/a|aka)\b", s, maxsplit=1, flags=re.IGNORECASE)[0]

    # Strip commas immediately preceding corporate suffix tokens.
    s = re.sub(r",\s*(?=\w)", " ", s)

    tokens = [t for t in re.split(r"\s+", s) if t]
    if not tokens:
        return ""

    out = []
    for t in tokens:
        bare = _strip_dots(t)
        low = bare.lower()
        if low in CORP_SUFFIXES:
            # Re-emit canonicalized suffix without dots, uppercase if 2-3 letters,
            # else titlecase. (LLC, Inc, Corp, Ltd...)
            if len(bare) <= 3:
                out.append(bare.upper())
            else:
                out.append(bare.capitalize())
        elif bare == "&":
            out.append("&")
        else:
            out.append(_titlecase_token(bare))
    canonical = re.sub(r"\s+", " ", " ".join(out)).strip()
    return canonical


def ascii_fold(s: str) -> str:
    return "".join(
        c for c in unicodedata.normalize("NFKD", s)
        if not unicodedata.combining(c)
    )


# --- selftest ----------------------------------------------------------------

SELFTEST_PERSON = [
    ("Smith, John A.", "John A Smith"),
    ("JOHN SMITH JR", "John Smith Jr"),
    ("Hon. Maria de la Cruz", "Maria de la Cruz"),
    ("  Dr.   Jane   Doe  ", "Jane Doe"),
    ("O'Brien, Patrick", "Patrick O'Brien"),
    ("john a smith", "John A Smith"),
    ("Mr. John Smith III", "John Smith III"),
    ("Jones-Smith, Mary", "Mary Jones-Smith"),
    ("", ""),
]

SELFTEST_ORG = [
    ("ACME Construction, LLC", "Acme Construction LLC"),
    ("The Tampa Tribune", "Tampa Tribune"),
    ("O'Brien & Sons Co.", "O'Brien & Sons Co"),
    ("IBM", "IBM"),
    ("acme inc", "Acme Inc"),
    ("ACME Construction LLC dba Acme Builders", "Acme Construction LLC"),
]


def selftest() -> int:
    failed = 0
    for inp, want in SELFTEST_PERSON:
        got = normalize_person(inp)
        ok = got == want
        print(f"[person] {'OK ' if ok else 'FAIL'}  {inp!r:48} -> {got!r:32} (want {want!r})")
        if not ok:
            failed += 1
    for inp, want in SELFTEST_ORG:
        got = normalize_org(inp)
        ok = got == want
        print(f"[org]    {'OK ' if ok else 'FAIL'}  {inp!r:48} -> {got!r:32} (want {want!r})")
        if not ok:
            failed += 1
    print(f"\n{failed} failures of {len(SELFTEST_PERSON) + len(SELFTEST_ORG)}")
    return 0 if failed == 0 else 1


def main(argv) -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--kind", choices=("person", "org"), default="person")
    p.add_argument("--name", default=None, help="if omitted, reads stdin")
    p.add_argument("--selftest", action="store_true")
    args = p.parse_args(argv[1:])

    if args.selftest:
        return selftest()

    name = args.name if args.name is not None else sys.stdin.read()
    fn = normalize_person if args.kind == "person" else normalize_org
    print(fn(name))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
