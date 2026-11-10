set positional-arguments

# List available commands.
default:
    @just --list

# Format Rust sources, or check formatting with --check.
fmt *args:
    cargo fmt --all -- "$@"

# Run Clippy with warnings treated as errors.
lint *args:
    cargo clippy --locked --all-targets "$@" -- -D warnings

# Run tests, optionally filtering by name or passing test runner arguments.
test *args:
    cargo test --locked "$@"

# Run formatting, lint, and test checks.
check: (fmt "--check") lint test

# Generate datasets with the release executable and optional arguments.
generate *args:
    cargo run --locked --release -- "$@"

# Remove build artifacts, cached resources, and generated outputs.
clean:
    cargo clean
    rm -rf assets output
