set positional-arguments

# List available commands.
default:
    @just --list

# Format Rust sources, or check formatting with --check.
fmt *args:
    cargo fmt --all -- "$@"

# Run Clippy with warnings treated as errors.
lint:
    cargo clippy --locked --all-targets -- -D warnings

# Run all tests.
test:
    cargo test --locked

# Run formatting, lint, and test checks.
check: (fmt "--check") lint test

# Run the debug executable with optional arguments.
run *args:
    cargo run --locked -- "$@"

# Generate datasets with the release executable and optional arguments.
generate *args:
    cargo run --locked --release -- "$@"

# Remove build artifacts, cached resources, and generated outputs.
clean:
    cargo clean
    rm -rf assets output
