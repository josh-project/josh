#!/usr/bin/env bash

set -euo pipefail
shopt -s inherit_errexit

function _run() {
  lang="${1}"
  out="${2}"

  declare repo_root
  repo_root=$(git rev-parse --show-toplevel)
  cd "${repo_root}"/docs

  mdbook-mermaid install "${repo_root}"/docs

  if [[ "${lang}" = "en_001" ]]; then
    mdbook build -d "${out}"
  else
    if [[ ! -f "po/${lang}.po" ]]; then
      echo "po/${lang}.po does not exist" >&2
      exit 1
    fi

    MDBOOK_BOOK__LANGUAGE="${lang}" mdbook build -d "${out}"
  fi
}

if [[ "$#" -lt 2 ]]; then
  cat >&2 <<'EOF'
Usage: build-docs.sh <lang> <output-subdir>
    lang             Locale code, e.g. "en_001" or "zh_CN". Use "en_001" to build
                     the untranslated English source.
    output-subdir    Path under docs/ to write the html output to,
                     e.g. "book" or "book/zh_CN".
EOF
  exit 1
fi

_run "${1}" "${2}"
