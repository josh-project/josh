"""Per-tool benchmark runners.

Each runner takes the tool's *native* input (a josh filter spec, git-filter-repo
flags, a copybara config, a filter-branch script), performs the run on a
cold-start copy of the source repo, times it, and returns a `ToolResult`.
Verification of the output shape is left to the scenario.
"""
