"""Rendering benchmark results as a log-scale comparison chart."""

import json
import math
from collections.abc import Callable
from pathlib import Path

import altair as alt
import pandas as pd

from bench.duration import format_duration
from bench.result import ToolResult

# Log-scale bars need an explicit, non-zero baseline; these bracket the range of
# elapsed times we expect to plot (sub-second to several minutes).
LOG_FLOOR = 0.1
LOG_CEIL = 500


def _decade_ticks(floor: float, ceil: float) -> list[float]:
    """Major ticks: one per power of ten within [floor, ceil]."""
    lo, hi = math.floor(math.log10(floor)), math.ceil(math.log10(ceil))
    return [10 ** e for e in range(lo, hi + 1) if floor <= 10 ** e <= ceil]


def _label_expr_from(ticks: list[float], fmt: Callable[[float], str]) -> str:
    """Vega expression that maps each tick value to its formatted label."""
    return json.dumps({str(t): fmt(t) for t in ticks}) + "[datum.value + '']"


def _minor_grid(
    scale: alt.Scale,
    floor: float,
    ceil: float,
    per_decade: int = 2,
    opacity: float = 0.12,
) -> alt.Chart:
    """Faint minor gridlines: `per_decade` rules per decade, evenly spaced in log space."""
    fracs = [(i + 1) / (per_decade + 1) for i in range(per_decade)]
    lo, hi = math.floor(math.log10(floor)), math.ceil(math.log10(ceil))
    ticks = [
        10 ** (e + f)
        for e in range(lo, hi)
        for f in fracs
        if floor <= 10 ** (e + f) <= ceil
    ]
    return alt.Chart(pd.DataFrame({"y": ticks})).mark_rule(
        color="gray", opacity=opacity, strokeWidth=0.5,
    ).encode(
        y=alt.Y("y:Q", scale=scale, axis=None),
    )


def _bars(
    data: pd.DataFrame,
    scale: alt.Scale,
    floor: float,
    ticks: list[float],
    order: list[str],
) -> alt.Chart:
    """Log-scale bars anchored to `floor`, with humanized axis labels and tooltip."""
    return alt.Chart(data).mark_bar().encode(
        x=alt.X("tool:N", sort=order, title="Tool"),
        y=alt.Y(
            "elapsed_s:Q",
            scale=scale,
            axis=alt.Axis(
                values=ticks,
                labelExpr=_label_expr_from(ticks, format_duration),
                grid=True,
                title="Elapsed time (log scale)",
            ),
        ),
        y2=alt.Y2(datum=floor),
        color=alt.Color("tool:N", legend=None),
        tooltip=[
            alt.Tooltip("tool:N", title="Tool"),
            alt.Tooltip("elapsed_human:N", title="Elapsed"),
        ],
    )


def _bar_labels(data: pd.DataFrame, scale: alt.Scale, order: list[str]) -> alt.Chart:
    """Value labels drawn just above each bar (for static/non-interactive export)."""
    return alt.Chart(data).mark_text(dy=-6, baseline="bottom", fontSize=12).encode(
        x=alt.X("tool:N", sort=order, axis=None),
        y=alt.Y("elapsed_s:Q", scale=scale, axis=None),
        text=alt.Text("elapsed_human:N"),
    )


def comparison_chart(results: list[ToolResult]) -> alt.LayerChart:
    """Build the log-scale bar chart comparing a list of ToolResults.

    Bars keep the order of `results`.
    """
    order = [r.name for r in results]
    data = pd.DataFrame({
        "tool": order,
        "elapsed_s": [r.elapsed for r in results],
    })
    data["elapsed_human"] = data["elapsed_s"].map(format_duration)

    scale = alt.Scale(type="log", domain=[LOG_FLOOR, LOG_CEIL])
    ticks = _decade_ticks(LOG_FLOOR, LOG_CEIL)

    # Independent y scales (identical defs) keep Vega-Lite from merging &
    # clobbering the bars axis when the gridline/label layers set axis=None.
    return alt.layer(
        _minor_grid(scale, LOG_FLOOR, LOG_CEIL),
        _bars(data, scale, LOG_FLOOR, ticks, order),
        _bar_labels(data, scale, order),
    ).resolve_scale(x="independent", y="independent").properties(
        title="Git filtering benchmarks (lower is better)",
        width=500,
    )


def save_chart(chart: alt.LayerChart, path: str | Path) -> Path:
    """Render `chart` to `path` (PNG via vl-convert), creating parent dirs."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    chart.save(str(path))
    return path
