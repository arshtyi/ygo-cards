# ygo-cards

Generate normalized OT and RD Yu-Gi-Oh! card datasets from upstream YGOPro resources.

## Usage

```sh
Available recipes:
    build          # Build the debug executable.
    check          # Run formatting, lint, and test checks.
    clean          # Remove Cargo build artifacts.
    default        # List available commands.
    fmt            # Format Rust sources.
    generate *args # Generate datasets with the release executable and optional arguments.
    lint           # Run Clippy with warnings treated as errors.
    run *args      # Run the debug executable with optional arguments.
    test           # Run all tests.
```

## Outputs

- `output/ot.json`, `output/rd.json`: card datasets sorted by ID.
- `output/report.md`: dataset summary, added/modified/removed counts, image checks, and diagnostics.
- `output/ot.diff`, `output/rd.diff`: separate complete unified diffs against the previous release; each is generated and attached only when its dataset changes.
- `output/release-notes.md`: concise release summary with added/modified/removed counts.
- `output/release-changed`: publication decision from the same comparison (`true`/`false`).
- `output/build.log`: structured build warnings and errors.

## Data

- Field definitions: [ygo-definitions](https://github.com/arshtyi/ygo-definitions).
- Field mappings: `config/{ot,rd}-field-mappings.json`.
- Resource, search, release, and image URLs: `config/endpoints.json`.

## Workflows

- [CI](.github/workflows/ci.yml): formatting, Clippy, and tests.
- [Publish](.github/workflows/publish-data.yml): manual or Monday/Friday at 10:00 UTC; checks images and publishes changed datasets as the next `0.0.x` release.
