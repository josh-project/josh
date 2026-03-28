#!/usr/bin/env python3
import sys
import json

for line in map(str.rstrip, sys.stdin):
    m = json.loads(line)
    try:
        for s in m["message"]["spans"]:
            print("{}:{}: {}".format(s["file_name"], s["line_start"], m["message"]["message"]))
    except:
        pass
