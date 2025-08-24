# this make file is just a small wrapper/helper to easily use this workspace within vim
#
# 1. Build workspace within vim and retrieve compile errors + warnings
# :make c
# 2. Format the code
# :make  f

t: c
	sh run-tests.sh tests/filter/*.t

c:
	cargo check --workspace --message-format=json | python3 ./scripts/rerr.py

f:
	cargo fmt

build-image-release:
	docker buildx build \
		--target=run \
		--build-context=git=.git \
		--tag josh-run .
