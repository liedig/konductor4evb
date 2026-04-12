.PHONY: build test check deploy clean

build:
	sam build --beta-features -c

test:
	cargo test --workspace

check:
	cargo check --workspace
	cargo clippy --workspace -- -D warnings

deploy: build
	sam deploy

clean:
	cargo clean
	rm -rf .aws-sam
