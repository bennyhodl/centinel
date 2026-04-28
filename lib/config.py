"""Read Centinel's doge.config.yaml + .env and resolve $WIKI / $CITY_SLUG / etc.

Source of truth for configuration. The web app and bin/centinel both read here.
"""
from __future__ import annotations

import os
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

try:
    import yaml
except ImportError as exc:
    print("ERROR: PyYAML required. Install with: pip install pyyaml", file=sys.stderr)
    raise SystemExit(1) from exc


REPO_ROOT = Path(__file__).resolve().parent.parent
CONFIG_PATH = REPO_ROOT / "doge.config.yaml"
ENV_PATH = REPO_ROOT / ".env"


@dataclass
class CityConfig:
    name: str
    slug: str
    domain: str
    timezone: str = "America/New_York"


@dataclass
class CronSchedules:
    sitemap_lint: str = "0 3 * * 1"      # Monday 03:00
    watch_runner: str = "0 4 * * *"       # Daily 04:00
    data_reporter: str = "0 */6 * * *"    # Every 6h
    vault_manifest: str = "*/15 * * * *"  # Every 15m
    huddle_rollup: str = "0 18 * * *"     # Daily 18:00
    briefings: str = "0 9 * * 1"          # Monday 09:00
    investigator_tick: str = "0 */4 * * *"  # Every 4h


@dataclass
class CentinelConfig:
    city: CityConfig
    wiki_path: Path
    cron: CronSchedules
    watch_presets: list[str]
    confidential_investigations: list[str]

    @property
    def db_path(self) -> Path:
        return self.wiki_path / "_data" / f"{self.city.slug}.db"

    @property
    def runtime_dir(self) -> Path:
        return self.wiki_path / "_runtime"

    @property
    def inbox_dir(self) -> Path:
        return self.runtime_dir / "inbox"

    @property
    def outbox_dir(self) -> Path:
        return self.runtime_dir / "outbox"

    @property
    def status_dir(self) -> Path:
        return self.runtime_dir / "status"


def load() -> CentinelConfig:
    """Load and validate doge.config.yaml. Raises SystemExit on missing file."""
    if not CONFIG_PATH.exists():
        print(
            f"ERROR: {CONFIG_PATH} not found. Run ./bootstrap to create it from the example.",
            file=sys.stderr,
        )
        raise SystemExit(1)

    with CONFIG_PATH.open() as f:
        raw: dict[str, Any] = yaml.safe_load(f) or {}

    city_raw = raw.get("city") or {}
    for required in ("name", "slug", "domain"):
        if not city_raw.get(required):
            print(f"ERROR: doge.config.yaml is missing city.{required}", file=sys.stderr)
            raise SystemExit(1)

    wiki_raw = (raw.get("wiki") or {}).get("path") or "~/wiki/Centinel"
    wiki_path = Path(os.path.expanduser(os.path.expandvars(wiki_raw))).resolve()

    cron_overrides = raw.get("cron_schedule_overrides") or {}
    cron = CronSchedules(**{k: v for k, v in cron_overrides.items() if hasattr(CronSchedules, k)})

    return CentinelConfig(
        city=CityConfig(
            name=city_raw["name"],
            slug=city_raw["slug"],
            domain=city_raw["domain"],
            timezone=city_raw.get("timezone", "America/New_York"),
        ),
        wiki_path=wiki_path,
        cron=cron,
        watch_presets=list(raw.get("watch_presets") or []),
        confidential_investigations=list(raw.get("confidential_investigations") or []),
    )


# Roles that get their own Hermes profile.
PROFILE_ROLES = ["investigator", "archivist", "data-reporter", "watch-runner"]

# Map role → skill loaded into that profile.
ROLE_SKILL = {
    "investigator": "civic-investigator",
    "archivist": "civic-archivist",
    "data-reporter": "civic-data-reporter",
    "watch-runner": "civic-watch-runner",
}
