"""Timing a block of code for benchmark runners."""

import time


class Timer:
    """Context manager measuring the wall-clock time of the block it wraps.

    `elapsed` is ``0.0`` inside the ``with`` block and is set to the elapsed
    seconds on exit (including when an exception propagates out)::

        with Timer() as t:
            do_work()
        print(t.elapsed)
    """

    __slots__ = ("_start", "elapsed")

    def __init__(self) -> None:
        self._start = 0.0
        self.elapsed = 0.0

    def __enter__(self) -> "Timer":
        self._start = time.perf_counter()
        return self

    def __exit__(
        self,
        exc_type: object,
        exc_value: object,
        traceback: object,
    ) -> None:
        self.elapsed = time.perf_counter() - self._start
