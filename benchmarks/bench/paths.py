"""Project locations."""

from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
TARGET_DIR = PROJECT_ROOT / "target"    # fetched repos, builds, working copies
OUTPUT_DIR = PROJECT_ROOT / "target"    # rendered charts
