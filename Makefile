# Shortcuts. `make` installs what is missing (first time) and opens the editor.
.PHONY: run cli bundle test check
run:
	./run.sh
cli:
	./run.sh cli --help
bundle:            ## macOS: dist/captainE.app
	sh tools/bundle.sh
test:
	cargo test --workspace --release
check:
	cargo fmt --all -- --check && cargo clippy --workspace --all-targets && cargo test --workspace --release
