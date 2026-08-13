.PHONY: dev api web fmt lint test build list-policies

dev:
	docker compose up --build

api:
	cd apps/governance-api && cargo run -- start

web:
	pnpm --dir apps/web dev

fmt:
	cargo fmt --all
	pnpm --dir apps/web exec eslint . --fix

lint:
	cargo clippy --workspace --all-targets --locked -- -D warnings
	pnpm --dir apps/web lint

test:
	cargo test --workspace --locked
	pnpm --dir apps/web test

build:
	cargo build --workspace --release --locked
	pnpm --dir apps/web build

list-policies:
	cargo run -p gov-eval -- list-policies --api-url http://127.0.0.1:8080
