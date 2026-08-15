"""select_folders: extract tests/ and docs/ under a josh/ prefix.

Every tool performs the same operation -- keep only the `tests/` and `docs/`
subtrees and re-root them under `josh/` -- expressed in that tool's own native
language. josh-filter is the subject; the other three are the comparison set.
"""

from bench.build import build_josh_filter
from bench.chart import comparison_chart, save_chart
from bench.git import fetch_repo, verify_result
from bench.paths import OUTPUT_DIR, TARGET_DIR
from bench.result import ToolResult
from bench.tools.copybara import run_copybara
from bench.tools.git_filter_branch import run_git_filter_branch
from bench.tools.git_filter_repo import run_git_filter_repo
from bench.tools.josh_filter import run_josh_filter

JOSH_REMOTE = "https://github.com/josh-project/josh"
JOSH_COMMIT = "43ed8cb6e905730f4ccd261ba8bc03cddea6c78d"

# josh-filter: extract tests/ and docs/ from the repo root, place both under a
# josh/ prefix.
#   :/tests  selects the tests/ subdirectory
#   :/docs   selects the docs/ subdirectory
#   :[<path> = <filter>, ...] combines sub-filters into named output paths
# So josh/tests = :/tests produces josh/tests/<contents of tests/>, etc.
JOSH_FILTER_SPEC = ":[josh/tests = :/tests, josh/docs = :/docs]"

# git-filter-repo:
#   --path tests/ --path docs/         keep only those subtrees
#   --path-rename tests/:josh/tests/   move tests/ -> josh/tests/
#   --path-rename docs/:josh/docs/     move docs/  -> josh/docs/
#   --force                            allow running on a fresh clone
GIT_FILTER_REPO_ARGS = (
    "--path tests/ --path docs/ "
    "--path-rename tests/:josh/tests/ "
    "--path-rename docs/:josh/docs/ "
    "--force"
)

# copybara migrates from origin -> destination; origin_files selects the subtrees
# and core.move re-roots them under josh/. {origin_url}/{dest_url} are filled in
# by the runner.
COPYBARA_CONFIG = """
core.workflow(
    name = "default",
    origin = git.origin(
        url = "file://{origin_url}",
        ref = "HEAD",
    ),
    origin_files = glob(["tests/**", "docs/**"]),
    destination = git.destination(
        url = "file://{dest_url}",
        fetch = "refs/heads/main",
        push = "refs/heads/main",
    ),
    authoring = authoring.pass_thru(default = "Copybara <copybara@example.com>"),
    mode = "ITERATIVE",
    transformations = [
        core.move("tests", "josh/tests"),
        core.move("docs", "josh/docs"),
    ],
)
"""

# git filter-branch --tree-filter: in each commit's working tree, move the two
# subtrees under josh/ and delete everything else at the top level.
TREE_FILTER_SCRIPT = (
    "mkdir -p josh\n"
    "if [ -d tests ]; then mv tests josh/tests; fi\n"
    "if [ -d docs ]; then mv docs josh/docs; fi\n"
    "find . -maxdepth 1 "
    "-not -name . -not -name .git -not -name josh "
    "-exec rm -rf {} +\n"
)


def run() -> list[ToolResult]:
    """Build josh, run every tool on the josh repo, verify, and chart the result."""
    josh_bin = build_josh_filter(JOSH_COMMIT, TARGET_DIR)
    source = fetch_repo(JOSH_REMOTE, "josh", JOSH_COMMIT, TARGET_DIR)

    work = TARGET_DIR / "work" / "select_folders"
    results = [
        run_josh_filter(josh_bin, source, JOSH_FILTER_SPEC, work / "josh-filter"),
        run_git_filter_repo(source, GIT_FILTER_REPO_ARGS, work / "git-filter-repo"),
        run_copybara(source, COPYBARA_CONFIG, work / "copybara"),
        run_git_filter_branch(source, TREE_FILTER_SCRIPT, work / "git-filter-branch"),
    ]

    for result in results:
        verify_result(result, expected_top_level="josh")

    out = save_chart(comparison_chart(results), OUTPUT_DIR / "select_folders.png")
    print(f"chart written to {out}")
    return results
