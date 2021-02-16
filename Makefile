
t: c

c:
	cargo build --workspace --message-format=json | python3 ~/.bin/rerr.py

f:
	cargo fmt
