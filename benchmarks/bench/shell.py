"""Running shell commands."""

import subprocess


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
