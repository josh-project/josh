"""The result of running one tool in a benchmark."""

from dataclasses import dataclass
from pathlib import Path


@dataclass
class ToolResult:
    """A single tool's run: how long it took and where to find its output.

    `repo` and `ref` locate the filtered result so it can be verified.
    """

    name: str
    elapsed: float
    repo: Path
    ref: str
