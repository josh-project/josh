"""Run a benchmark scenario: `python -m bench <scenario>`."""

import sys

from bench.scenarios import SCENARIOS


def main(argv: list[str] | None = None) -> None:
    argv = sys.argv[1:] if argv is None else argv
    names = ", ".join(sorted(SCENARIOS))

    if len(argv) != 1:
        sys.exit(f"usage: python -m bench <scenario>\nscenarios: {names}")

    name = argv[0]
    scenario = SCENARIOS.get(name)
    if scenario is None:
        sys.exit(f"unknown scenario: {name!r}\nscenarios: {names}")

    scenario()


if __name__ == "__main__":
    main()
