This document describes a set of rules and conventions maintainers use in this repo.

## PR structure

* PRs in this repo are kept to one commit only. Iterations on the PR
  should be folded into existing commit by amending.

## Commit message conventions

* Use an `Assisted-By:` footer (not `Co-Authored-By:`) to attribute LLM/agent involvement in commits
* The `Assisted-By:` footer must reference the actual model used, not a generic name
  * Example footer: `Assisted-By: model-author/model-name-v1`

* Commit messages must not use a conventional commits prefix (e.g. no `fix:` or `feat:`)
* Commit subject line must start with an uppercase letter and be reasonably short (around 80 characters)

* Commit messages must include a `Change:` footer with an alphanumeric, dash-separated identifier
  * Example footer: `Change: flatten-invert-check`

## Committing work

* Always run `cargo fmt` when committing changes in Rust code.

## Agentic work

* For creating temporary plans, files, and experiments, use the gitignored `.agents/work/` folder
  in the root of the repo
* Inside, create subfolders matching to current work topic
  * Example folder: `.agents/work/gix-port`
  * Example file: `.agents/work/gix-port/GIX_PORT_PROGRESS.md`
