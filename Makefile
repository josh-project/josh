# this make file is just a small wrapper/helper to easily use this workspace within vim
#
# 1. Build workspace within vim and retrieve compile errors + warnings
# :make c
# 2. Format the code
# :make  f

t: c

c:
	cargo build --workspace --message-format=json | python3 ./scripts/rerr.py

f:
	cargo fmt
