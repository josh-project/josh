  $ export TESTTMP=${PWD}

The bundled skill is concise Agent Skills-compatible Markdown.

  $ josh agent skill print > printed.md
  $ head -4 printed.md
  ---
  name: josh
  description: Use Josh for filtered Git repositories, local workspaces, and structured automation.
  ---
  $ test "$(wc -w < printed.md)" -lt 500 && echo concise
  concise

Install the skill into any agent-specific skill directory.

  $ josh agent skill install --target skills/josh
  Installed Josh agent skill at ${TESTTMP}/skills/josh/SKILL.md
  $ cmp printed.md skills/josh/SKILL.md

Existing skills are protected unless --force is explicit.

  $ josh agent skill install --target skills/josh 2>&1
  Error: Agent skill '${TESTTMP}/skills/josh/SKILL.md' already exists; pass --force to replace it
  [1]

  $ josh --output json --quiet agent skill install --target skills/josh > error.json
  [1]
  $ python3 - <<'PY'
  > import json
  > data = json.load(open("error.json"))
  > print(data["command"], data["success"], data["error"]["code"])
  > print("messages" in data)
  > PY
  agent.skill.install False agent_skill.already_exists
  False

  $ echo stale > skills/josh/SKILL.md
  $ josh agent skill install --target skills/josh --force
  Updated Josh agent skill at ${TESTTMP}/skills/josh/SKILL.md
  $ cmp printed.md skills/josh/SKILL.md

Dry-run and machine modes expose deterministic structured results.

  $ josh agent skill install --target dry/josh --dry-run
  Would install Josh agent skill at ${TESTTMP}/dry/josh/SKILL.md
  $ test ! -e dry/josh/SKILL.md

  $ josh --output json --quiet agent skill print > skill.json
  $ python3 - <<'PY'
  > import json
  > data = json.load(open("skill.json"))
  > print(data["command"], data["data"]["name"], data["data"]["version"])
  > print("messages" in data, data["data"]["content"].startswith("---\nname: josh\n"))
  > PY
  agent.skill.print josh 1
  False True
