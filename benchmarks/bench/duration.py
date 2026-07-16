"""Human-readable durations for chart labels."""

from quantiphy import Quantity


def format_duration(seconds: float) -> str:
    """Compact duration: SI units below a minute (231 ms, 10 s), min+sec above."""
    if seconds < 60:
        return Quantity(seconds, "s").render(prec=2)
    minutes, secs = divmod(round(seconds), 60)
    return f"{minutes} min {secs} s" if secs else f"{minutes} min"
