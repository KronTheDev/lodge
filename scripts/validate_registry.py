#!/usr/bin/env python3
"""
Validate extensions/registry.json and extensions/official.json for Lodge.

Checks:
  - JSON is well-formed (both files)
  - schema field is present and is an integer
  - No duplicate extension IDs
  - No duplicate resolved aliases (alias field, falling back to id — mirrors command_alias())
  - status is one of: stable | preview | coming-soon
  - sha256 is present whenever payload_url is set (no unverified remote downloads)
  - sha256, when present, is a 64-character lowercase hex string
  - Every ID in official.json exists in registry.json

Exit codes:
  0 — valid
  1 — one or more errors found
"""

import json
import re
import sys
from pathlib import Path

REGISTRY_PATH = Path(__file__).parent.parent / "extensions" / "registry.json"
VALID_STATUSES = {"stable", "preview", "coming-soon"}
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")

errors: list[str] = []


def err(msg: str) -> None:
    errors.append(msg)


# ── Load ──────────────────────────────────────────────────────────────────────

try:
    raw = REGISTRY_PATH.read_text(encoding="utf-8")
except FileNotFoundError:
    print(f"ERROR: registry not found at {REGISTRY_PATH}")
    sys.exit(1)

try:
    registry = json.loads(raw)
except json.JSONDecodeError as e:
    print(f"ERROR: registry.json is not valid JSON — {e}")
    sys.exit(1)

# ── schema field ──────────────────────────────────────────────────────────────

if "schema" not in registry:
    err("missing top-level 'schema' field")
elif not isinstance(registry["schema"], int):
    err(f"'schema' must be an integer, got {type(registry['schema']).__name__}")

extensions = registry.get("extensions", [])
if not isinstance(extensions, list):
    err("'extensions' must be an array")
    print("\n".join(f"  {e}" for e in errors))
    sys.exit(1)

# ── Per-entry checks ──────────────────────────────────────────────────────────

seen_ids: dict[str, int] = {}       # id → first index
seen_aliases: dict[str, str] = {}   # resolved alias → id of first claimer

for i, entry in enumerate(extensions):
    if not isinstance(entry, dict):
        err(f"entry #{i} is not an object")
        continue

    entry_id = entry.get("id", f"<entry #{i}>")
    label = f"'{entry_id}'"

    # Required fields
    for field in ("id", "name", "version", "description", "status"):
        if field not in entry:
            err(f"{label}: missing required field '{field}'")

    # Duplicate ID
    if "id" in entry:
        if entry["id"] in seen_ids:
            err(
                f"{label}: duplicate id — also used by entry #{seen_ids[entry['id']]}"
            )
        else:
            seen_ids[entry["id"]] = i

    # Resolved alias (mirrors command_alias() in extensions.rs)
    resolved_alias = entry.get("alias") or entry.get("id", "")
    if resolved_alias:
        if resolved_alias in seen_aliases:
            err(
                f"{label}: alias collision — '{resolved_alias}' is already claimed "
                f"by '{seen_aliases[resolved_alias]}'"
            )
        else:
            seen_aliases[resolved_alias] = entry_id

    # status
    status = entry.get("status")
    if status is not None and status not in VALID_STATUSES:
        err(
            f"{label}: invalid status '{status}' — "
            f"must be one of: {', '.join(sorted(VALID_STATUSES))}"
        )

    # sha256 required when payload_url is set
    payload_url = entry.get("payload_url")
    sha256 = entry.get("sha256")
    if payload_url and not sha256:
        err(
            f"{label}: 'sha256' is required when 'payload_url' is set — "
            "unverified remote downloads are not allowed"
        )

    # sha256 format when present
    if sha256 and not SHA256_RE.match(sha256):
        err(
            f"{label}: 'sha256' must be a 64-character lowercase hex string, "
            f"got '{sha256[:20]}...'"
        )

# ── official.json cross-check ─────────────────────────────────────────────────

OFFICIAL_PATH = Path(__file__).parent.parent / "extensions" / "official.json"

try:
    official_raw = OFFICIAL_PATH.read_text(encoding="utf-8")
except FileNotFoundError:
    err(f"extensions/official.json not found — it must exist even if empty")
    official_raw = None

if official_raw is not None:
    try:
        official_data = json.loads(official_raw)
    except json.JSONDecodeError as e:
        err(f"official.json is not valid JSON — {e}")
        official_data = None

    if official_data is not None:
        official_ids = official_data.get("official", [])
        if not isinstance(official_ids, list):
            err("official.json: 'official' must be an array")
        else:
            known_ids = {e.get("id") for e in extensions if isinstance(e, dict)}
            for oid in official_ids:
                if not isinstance(oid, str):
                    err(f"official.json: entry {oid!r} is not a string")
                elif oid not in known_ids:
                    err(
                        f"official.json: '{oid}' is not in registry.json — "
                        "remove it or add the extension to the registry first"
                    )

# ── Report ────────────────────────────────────────────────────────────────────

if errors:
    print(f"registry validation failed — {len(errors)} error(s):\n")
    for e in errors:
        print(f"  {e}")
    sys.exit(1)
else:
    print(f"registry valid — {len(extensions)} extension(s), no issues found.")
    sys.exit(0)
