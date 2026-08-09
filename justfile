# List available commands.
default:
    @just --list

# Format Rust sources.
fmt:
    cargo fmt --all

# Run Clippy with warnings treated as errors.
lint:
    cargo clippy --locked --all-targets -- -D warnings

# Run all tests.
test:
    cargo test --locked

# Run formatting, lint, and test checks.
check:
    cargo fmt --all -- --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked

# Build the debug executable.
build:
    cargo build --locked

# Run the debug executable with optional arguments.
run *args:
    cargo run --locked -- {{args}}

# Generate datasets with the release executable and optional arguments.
generate *args:
    cargo run --locked --release -- {{args}}

# Remove Cargo build artifacts.
clean:
    cargo clean
