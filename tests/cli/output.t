Capabilities are available outside a repository and describe the machine contract.

  $ josh --output json capabilities > capabilities.json
  $ python3 - <<'PY'
  > import json
  > data = json.load(open("capabilities.json"))
  > print(data["schema_version"], data["type"], data["command"], data["success"])
  > print(",".join(data["data"]["output"]["formats"]))
  > PY
  1 result capabilities True
  human,json,jsonl
  $ wc -l < capabilities.json | tr -d ' '
  1

Brief, quiet output omits duplicated messages and minimizes context use.

  $ josh --output json --quiet capabilities --brief > brief.json
  $ python3 - <<'PY'
  > import json
  > data = json.load(open("brief.json"))
  > print(data["data"]["schema_version"], data["data"]["agent_skill"])
  > print("messages" in data, sorted(data["data"]))
  > PY
  1 True
  False ['agent_skill', 'output_formats', 'schema_version', 'version', 'workspaces']
  $ wc -l < brief.json | tr -d ' '
  1

Pretty output remains available for interactive inspection.

  $ josh --output json --pretty capabilities --brief > pretty.json
  $ test "$(wc -l < pretty.json)" -gt 1 && echo pretty
  pretty

Filter validation and explanation also work outside a repository.

  $ josh --output json filter explain ':/src:prefix=app' > filter.json
  $ python3 - <<'PY'
  > import json
  > data = json.load(open("filter.json"))
  > print(data["command"], data["data"]["valid"], data["data"]["filter"])
  > PY
  filter.explain True app = :/src

Shell completions are generated without entering a repository.

  $ josh completions bash | grep -q '^_josh()' && echo found
  found

Machine-readable workspace results contain typed data and capture human messages.

  $ git init -q repo
  $ cd repo
  $ git commit -q --allow-empty -m initial
  $ josh workspace create ws --map src=:/src >/dev/null
  $ josh --output json workspace list > list.json
  $ python3 - <<'PY'
  > import json
  > data = json.load(open("list.json"))
  > print(data["command"], data["success"], len(data["data"]))
  > print(data["data"][0]["path"], data["data"][0]["valid"])
  > print(data["messages"][0]["level"])
  > PY
  workspace.list True 1
  ws True
  result

JSONL emits messages followed by one final result.

  $ josh --output jsonl workspace list > list.jsonl
  $ python3 - <<'PY'
  > import json
  > rows = [json.loads(line) for line in open("list.jsonl")]
  > print([row["type"] for row in rows])
  > print(rows[-1]["command"], rows[-1]["success"])
  > PY
  ['message', 'result']
  workspace.list True

Runtime and argument errors are structured and use non-zero exit statuses.

  $ josh --output json --quiet workspace show missing >error.json 2>stderr.txt
  [1]
  $ test ! -s stderr.txt
  $ python3 - <<'PY'
  > import json
  > data = json.load(open("error.json"))
  > print(data["command"], data["success"], data["error"]["code"])
  > print("messages" in data)
  > PY
  workspace.show False workspace.not_found
  False

  $ josh --output json workspace show >usage.json 2>stderr.txt
  [2]
  $ test ! -s stderr.txt
  $ python3 - <<'PY'
  > import json
  > data = json.load(open("usage.json"))
  > print(data["command"], data["success"], data["error"]["code"])
  > PY
  workspace.show False cli.usage
