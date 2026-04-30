"""
Centinel service management — install / status / restart / logs / deploy.

The whole stack runs on the host now (no Docker):
  - centinel-web.service       Next.js web app (port 3000)
  - centinel-datasette.service Read-only DB browser (port 8001, localhost-only)
  - hermes (separate)          Runs the agent + cron daemon

Services are installed as systemd --user units so restarts don't need sudo.
Make them survive logout with: `loginctl enable-linger $USER`.
"""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import sys
from pathlib import Path
from typing import Optional
from urllib.error import URLError
from urllib.request import urlopen


REPO_ROOT = Path(__file__).resolve().parent.parent
SYSTEMD_SRC = REPO_ROOT / "deploy" / "systemd"
SYSTEMD_DST = Path.home() / ".config" / "systemd" / "user"

WEB_UNIT = "centinel-web.service"
DATASETTE_UNIT = "centinel-datasette.service"
ALL_UNITS = (WEB_UNIT, DATASETTE_UNIT)

DATASETTE_VENV = Path("/opt/centinel/datasette-venv")


# ─── tiny output helpers ──────────────────────────────────────────────────────


def _green(s: str) -> str:  return f"\033[32m{s}\033[0m"
def _red(s: str) -> str:    return f"\033[31m{s}\033[0m"
def _yellow(s: str) -> str: return f"\033[33m{s}\033[0m"
def _dim(s: str) -> str:    return f"\033[2m{s}\033[0m"


def _info(msg: str) -> None: print(f"→ {msg}")
def _ok(msg: str) -> None:   print(_green(f"✓ {msg}"))
def _warn(msg: str) -> None: print(_yellow(f"⚠ {msg}"))
def _err(msg: str) -> None:  print(_red(f"✗ {msg}"), file=sys.stderr)


# ─── systemd helpers ──────────────────────────────────────────────────────────


def _systemctl(*args: str, capture: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["systemctl", "--user", *args],
        check=False,
        text=True,
        capture_output=capture,
    )


def _is_active(unit: str) -> bool:
    return _systemctl("is-active", unit).stdout.strip() == "active"


def _is_enabled(unit: str) -> bool:
    return _systemctl("is-enabled", unit).stdout.strip() == "enabled"


def _port_open(port: int, host: str = "127.0.0.1", timeout: float = 1.0) -> bool:
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def _http_ok(url: str, timeout: float = 2.0) -> tuple[bool, Optional[int]]:
    try:
        with urlopen(url, timeout=timeout) as r:
            return (200 <= r.status < 500, r.status)
    except URLError:
        return (False, None)
    except Exception:
        return (False, None)


# ─── install-services ─────────────────────────────────────────────────────────


def cmd_install_services(args) -> int:
    """Install systemd --user units and start the services."""
    if not shutil.which("systemctl"):
        _err("systemctl not found — this command requires systemd.")
        return 2

    SYSTEMD_DST.mkdir(parents=True, exist_ok=True)

    # 1. Datasette venv (idempotent)
    _ensure_datasette_venv()

    # 2. Copy units
    for unit in ALL_UNITS:
        src = SYSTEMD_SRC / unit
        dst = SYSTEMD_DST / unit
        if not src.exists():
            _err(f"unit template missing: {src}")
            return 1
        dst.write_text(src.read_text())
        _ok(f"installed {dst}")

    # 3. Reload + enable + start
    rc = _systemctl("daemon-reload").returncode
    if rc != 0:
        _err("systemctl daemon-reload failed")
        return rc

    for unit in ALL_UNITS:
        rc = _systemctl("enable", "--now", unit).returncode
        if rc != 0:
            _err(f"failed to enable+start {unit}")
            return rc
        _ok(f"enabled and started {unit}")

    print()
    _info("To make services survive logout (needed on EC2):")
    print(_dim("    sudo loginctl enable-linger $USER"))
    print()
    _info("Tail logs with:")
    print(_dim("    journalctl --user -u centinel-web -f"))
    print(_dim("    journalctl --user -u centinel-datasette -f"))
    return 0


def _ensure_datasette_venv() -> None:
    """Create /opt/centinel/datasette-venv with datasette installed."""
    if (DATASETTE_VENV / "bin" / "datasette").exists():
        _ok(f"datasette venv already at {DATASETTE_VENV}")
        return

    parent = DATASETTE_VENV.parent
    if not parent.exists():
        _info(f"creating {parent} (needs sudo)")
        rc = subprocess.run(
            ["sudo", "mkdir", "-p", str(parent)],
            check=False,
        ).returncode
        if rc != 0:
            _warn(f"could not create {parent}; falling back to ~/.local/bin/datasette")
            _ensure_datasette_pipx()
            return
        # Hand ownership to current user so future updates don't need sudo
        subprocess.run(["sudo", "chown", f"{os.getuid()}:{os.getgid()}", str(parent)], check=False)

    _info(f"creating venv at {DATASETTE_VENV}")
    rc = subprocess.run(
        [sys.executable, "-m", "venv", str(DATASETTE_VENV)],
        check=False,
    ).returncode
    if rc != 0:
        _warn("venv creation failed; falling back to pipx")
        _ensure_datasette_pipx()
        return

    pip = DATASETTE_VENV / "bin" / "pip"
    _info("installing datasette into venv")
    rc = subprocess.run(
        [str(pip), "install", "--upgrade", "pip", "datasette"],
        check=False,
    ).returncode
    if rc != 0:
        _err("datasette install failed")
        return
    _ok(f"datasette installed at {DATASETTE_VENV}/bin/datasette")


def _ensure_datasette_pipx() -> None:
    if shutil.which("datasette"):
        _ok("datasette already on PATH")
        return
    if not shutil.which("pipx"):
        _err("Neither datasette nor pipx is installed. Install pipx or run:")
        print(_dim("    python3 -m pip install --user datasette"))
        return
    rc = subprocess.run(["pipx", "install", "datasette"], check=False).returncode
    if rc == 0:
        _ok("datasette installed via pipx")


# ─── status ───────────────────────────────────────────────────────────────────


def cmd_status(args) -> int:
    """Show status of web, datasette, and Hermes."""
    rows: list[tuple[str, str, str]] = []  # (label, status_text, detail)

    # Web
    web_active = _is_active(WEB_UNIT)
    web_port = _port_open(3000)
    web_http_ok, web_status = _http_ok("http://127.0.0.1:3000/", timeout=2)
    if web_active and web_port and web_http_ok:
        rows.append(("centinel-web", _green("● active"), f"http://localhost:3000 (HTTP {web_status})"))
    elif web_active and not web_port:
        rows.append(("centinel-web", _yellow("● active, port closed"), "service running but :3000 not listening"))
    elif web_active:
        rows.append(("centinel-web", _yellow("● active, no HTTP"), f":3000 open, response={web_status}"))
    else:
        rows.append(("centinel-web", _red("○ inactive"), _dim(f"systemctl --user start {WEB_UNIT}")))

    # Datasette
    ds_active = _is_active(DATASETTE_UNIT)
    ds_port = _port_open(8001)
    ds_http_ok, ds_status = _http_ok("http://127.0.0.1:8001/-/versions.json", timeout=2)
    if ds_active and ds_port and ds_http_ok:
        rows.append(("centinel-datasette", _green("● active"), f"http://localhost:8001 (HTTP {ds_status})"))
    elif ds_active:
        rows.append(("centinel-datasette", _yellow("● active, not responding"), f":8001 open={ds_port}, http={ds_status}"))
    else:
        rows.append(("centinel-datasette", _red("○ inactive"), _dim(f"systemctl --user start {DATASETTE_UNIT}")))

    # Hermes API
    hermes_url = os.environ.get("HERMES_API_URL", "http://localhost:8000/v1").rstrip("/")
    base = hermes_url[:-3] if hermes_url.endswith("/v1") else hermes_url
    hermes_ok, hermes_status = _http_ok(f"{base}/health", timeout=2)
    if hermes_ok:
        rows.append(("hermes (api)", _green("● reachable"), f"{base}/health → {hermes_status}"))
    else:
        rows.append(("hermes (api)", _red("○ unreachable"), _dim(f"check daemon + API_SERVER_ENABLED=1")))

    # Hermes cron jobs
    cron_count = _hermes_cron_count()
    if cron_count is not None:
        rows.append(("hermes (cron)", _green("● ok") if cron_count > 0 else _yellow("● 0 jobs"),
                     f"{cron_count} centinel-owned job(s)"))

    # Wiki
    wiki_path = _resolve_wiki_path()
    if wiki_path and wiki_path.exists():
        inv_dir = wiki_path / "Investigations"
        inv_count = len(list(inv_dir.glob("*.md"))) if inv_dir.exists() else 0
        rows.append(("wiki", _green("● ok"), f"{wiki_path} ({inv_count} investigation(s))"))
    else:
        rows.append(("wiki", _red("○ missing"), str(wiki_path)))

    # Print table
    print()
    label_w = max(len(r[0]) for r in rows)
    for label, status, detail in rows:
        print(f"  {label.ljust(label_w)}  {status}  {detail}")
    print()

    # Exit non-zero if anything is broken
    bad = any(_red("○") in s for _, s, _ in rows)
    return 1 if bad else 0


def _resolve_wiki_path() -> Optional[Path]:
    import contextlib, io
    try:
        from lib.config import load
        with contextlib.redirect_stderr(io.StringIO()):
            return load().wiki_path
    except (Exception, SystemExit):
        env = os.environ.get("CENTINEL_WIKI_PATH")
        return Path(os.path.expanduser(env)).resolve() if env else None


def _hermes_cron_count() -> Optional[int]:
    """Read centinel-prefixed jobs straight from jobs.json (no shell-out)."""
    paths = [
        Path.home() / ".hermes" / "profiles" / "investigator" / "cron" / "jobs.json",
        Path.home() / ".hermes" / "cron" / "jobs.json",
    ]
    total = 0
    found_any = False
    for p in paths:
        if not p.exists():
            continue
        found_any = True
        try:
            data = json.loads(p.read_text())
        except Exception:
            continue
        for job in data.get("jobs", []):
            if (job.get("name") or "").startswith("centinel-"):
                total += 1
    return total if found_any else None


# ─── restart / logs / deploy ──────────────────────────────────────────────────


_TARGETS = {
    "web": [WEB_UNIT],
    "datasette": [DATASETTE_UNIT],
    "all": list(ALL_UNITS),
}


def cmd_restart(args) -> int:
    units = _TARGETS.get(args.target, list(ALL_UNITS))
    for u in units:
        rc = _systemctl("restart", u, capture=False).returncode
        if rc != 0:
            _err(f"restart failed: {u}")
            return rc
        _ok(f"restarted {u}")
    return 0


def cmd_logs(args) -> int:
    units = _TARGETS.get(args.target, list(ALL_UNITS))
    cmd = ["journalctl", "--user"]
    for u in units:
        cmd += ["-u", u]
    if args.follow:
        cmd.append("-f")
    if args.lines:
        cmd += ["-n", str(args.lines)]
    return subprocess.run(cmd, check=False).returncode


def cmd_deploy(args) -> int:
    """git pull + pnpm install + build + restart."""
    steps: list[tuple[str, list[str], dict]] = [
        ("git pull",         ["git", "pull", "--ff-only"],           {"cwd": str(REPO_ROOT)}),
        ("pnpm install",     ["pnpm", "install", "--frozen-lockfile"], {"cwd": str(REPO_ROOT / "app")}),
        ("pnpm build",       ["pnpm", "build"],                      {"cwd": str(REPO_ROOT / "app")}),
    ]
    for label, cmd, kwargs in steps:
        _info(label)
        rc = subprocess.run(cmd, check=False, **kwargs).returncode
        if rc != 0:
            _err(f"step failed: {label}")
            return rc

    for u in ALL_UNITS:
        _systemctl("restart", u, capture=False)
        _ok(f"restarted {u}")
    return 0


# ─── argparse wiring ──────────────────────────────────────────────────────────


def register_subcommands(sub) -> None:
    sub.add_parser(
        "install-services",
        help="Install systemd --user units for web + datasette",
    ).set_defaults(func=cmd_install_services)

    sub.add_parser(
        "status",
        help="Show health of web, datasette, hermes, wiki",
    ).set_defaults(func=cmd_status)

    sp = sub.add_parser("restart", help="Restart a service")
    sp.add_argument("target", nargs="?", default="all",
                    choices=list(_TARGETS), help="which service to restart")
    sp.set_defaults(func=cmd_restart)

    sp = sub.add_parser("logs", help="Tail service logs")
    sp.add_argument("target", nargs="?", default="all",
                    choices=list(_TARGETS), help="which service")
    sp.add_argument("-f", "--follow", action="store_true", help="follow output")
    sp.add_argument("-n", "--lines", type=int, default=200, help="lines to show")
    sp.set_defaults(func=cmd_logs)

    sub.add_parser(
        "deploy",
        help="git pull + install + build + restart",
    ).set_defaults(func=cmd_deploy)
