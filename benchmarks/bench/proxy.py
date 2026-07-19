"""Running josh-proxy (plus a local git HTTP server) for benchmark scenarios.

josh-proxy only accepts http(s)/ssh remotes, so the upstream mirror is served
over local HTTP with the workspace's axum-cgi-server wrapping git http-backend,
mirroring ``tests/proxy/setup_test_env.sh``. The git HTTP server is usable on
its own (``GitHttpServer``) for tools that talk to the upstream directly, like
the josh CLI.
"""

import os
import signal
import socket
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

_READY_TIMEOUT = 60.0


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _wait_http(url: str, proc: subprocess.Popen, what: str, log: Path) -> None:
    """Poll `url` until it answers (any HTTP status) or `proc` dies."""
    deadline = time.monotonic() + _READY_TIMEOUT
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"{what} exited (status {proc.returncode}); log: {log}")
        try:
            with urllib.request.urlopen(url, timeout=1):
                return
        except urllib.error.HTTPError:
            return  # server is up; an error status is still an answer
        except OSError:
            time.sleep(0.1)
    raise RuntimeError(f"{what} did not become ready within {_READY_TIMEOUT}s; log: {log}")


class _Service:
    """Shared process-lifecycle plumbing for the servers below."""

    def __init__(self, log_dir: str | Path) -> None:
        self.log_dir = Path(log_dir)
        self._procs: list[tuple[str, subprocess.Popen]] = []
        self._logs: list[object] = []

    def _spawn(self, name: str, cmd: list[str], env: dict[str, str]) -> subprocess.Popen:
        self.log_dir.mkdir(parents=True, exist_ok=True)
        log = open(self.log_dir / f"{name}.log", "w")
        self._logs.append(log)
        proc = subprocess.Popen(cmd, env=env, stdout=log, stderr=subprocess.STDOUT)
        self._procs.append((name, proc))
        return proc

    def _shutdown(self) -> None:
        for _, proc in reversed(self._procs):
            if proc.poll() is None:
                proc.send_signal(signal.SIGTERM)
        deadline = time.monotonic() + 10
        for _, proc in self._procs:
            try:
                proc.wait(timeout=max(0.1, deadline - time.monotonic()))
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        for log in self._logs:
            log.close()
        self._procs, self._logs = [], []


class GitHttpServer(_Service):
    """Context manager serving the bare repos under `serve_root` over HTTP."""

    def __init__(
        self, binaries: dict[str, Path], serve_root: str | Path, log_dir: str | Path
    ) -> None:
        super().__init__(log_dir)
        self.binaries = binaries
        self.serve_root = Path(serve_root)
        self.port = 0

    def __enter__(self) -> "GitHttpServer":
        self.port = _free_port()
        env = {
            **os.environ,
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_PROJECT_ROOT": str(self.serve_root),
            "GIT_HTTP_EXPORT_ALL": "1",
        }
        httpd = self._spawn("axum-cgi-server", [
            str(self.binaries["axum-cgi-server"]),
            f"--port={self.port}",
            f"--dir={self.serve_root}",
            "--cmd=git",
            "--args=http-backend",
        ], env)
        _wait_http(
            f"http://localhost:{self.port}/",
            httpd, "axum-cgi-server", self.log_dir / "axum-cgi-server.log",
        )
        return self

    def __exit__(self, *exc: object) -> None:
        self._shutdown()

    def url(self, repo: str) -> str:
        """Plain (unfiltered) HTTP URL for `repo`."""
        return f"http://localhost:{self.port}/{repo}.git"


class JoshProxy(_Service):
    """Context manager running axum-cgi-server + josh-proxy over `serve_root`.

    `serve_root` is a directory of bare repos; a repo at
    ``<serve_root>/rust-lang/rust.git`` is reachable through the proxy as
    ``http://localhost:<port>/rust-lang/rust.git<filter>.git``. `cache_dir` is
    josh-proxy's ``--local`` state: pass a fresh dir for a cold cache, keep it
    across syncs for a warm one.
    """

    def __init__(
        self,
        binaries: dict[str, Path],
        serve_root: str | Path,
        cache_dir: str | Path,
        log_dir: str | Path,
    ) -> None:
        super().__init__(log_dir)
        self.binaries = binaries
        self.serve_root = Path(serve_root)
        self.cache_dir = Path(cache_dir)
        self.git_server = GitHttpServer(binaries, serve_root, log_dir)
        self.josh_port = 0

    def __enter__(self) -> "JoshProxy":
        self.cache_dir.mkdir(parents=True, exist_ok=True)
        self.git_server.__enter__()

        self.josh_port = _free_port()
        proxy = self._spawn("josh-proxy", [
            str(self.binaries["josh-proxy"]),
            f"--port={self.josh_port}",
            f"--local={self.cache_dir}",
            f"--remote=http://localhost:{self.git_server.port}",
            "--no-background",
        ], {**os.environ, "GIT_CONFIG_NOSYSTEM": "1"})
        _wait_http(
            f"http://localhost:{self.josh_port}/",
            proxy, "josh-proxy", self.log_dir / "josh-proxy.log",
        )
        return self

    def __exit__(self, *exc: object) -> None:
        self._shutdown()
        self.git_server.__exit__(*exc)

    def url(self, repo: str, filter_spec: str, at: str | None = None) -> str:
        """Proxy URL for `repo` filtered by `filter_spec`, optionally at commit `at`.

        The filter is percent-encoded exactly as rustc-josh-sync encodes it.
        """
        rev = f"@{at}" if at else ""
        quoted = urllib.parse.quote(filter_spec, safe="")
        return f"http://localhost:{self.josh_port}/{repo}.git{rev}{quoted}.git"
