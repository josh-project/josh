"""Running shell commands."""

import os
import shlex
import subprocess
from collections.abc import Mapping, Sequence
from pathlib import Path


def run(cmd: str, cwd: str | None = None, echo: bool = False) -> str:
    """Run a shell command, show its output, raise on failure.

    Returns the command's stdout. Prints stdout when `echo` is set or the
    command fails; prints stderr and raises `RuntimeError` on a nonzero exit.
    """
    p = subprocess.run(
        cmd, shell=True, cwd=cwd,
        capture_output=True, text=True,
    )

    if echo or p.returncode != 0:
        print(p.stdout)

    if p.returncode != 0:
        print(p.stderr)
        raise RuntimeError(f"exit {p.returncode}: {cmd}")

    return p.stdout


def run_args(
    args: Sequence[str | Path],
    cwd: str | Path | None = None,
    *,
    env: Mapping[str, str] | None = None,
    stdin: str | None = None,
    echo: bool = False,
) -> str:
    """Run an argv without a shell and return stdout."""
    argv = [str(arg) for arg in args]
    process_env = os.environ.copy()
    if env:
        process_env.update(env)
    p = subprocess.run(
        argv,
        cwd=cwd,
        env=process_env,
        input=stdin,
        capture_output=True,
        text=True,
    )

    if echo or p.returncode != 0:
        print(p.stdout)

    if p.returncode != 0:
        print(p.stderr)
        raise RuntimeError(f"exit {p.returncode}: {shlex.join(argv)}")

    return p.stdout
