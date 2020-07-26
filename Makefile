
t: c w

c:
	cargo build --workspace --message-format=json | python3 ~/.bin/rerr.py

w: c
	cd josh-review; cargo make build

f:
	cargo fmt
