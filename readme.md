# ygo-cards

Generate normalized OT and RD Yu-Gi-Oh! card datasets from upstream YGOPro resources.

## Usage

```sh
Available recipes:
    check          # Run formatting, lint, and test checks.
    clean          # Remove build artifacts, cached resources, and generated outputs.
    default        # List available commands.
    fmt *args      # Format Rust sources, or check formatting with --check.
    generate *args # Generate datasets with the release executable and optional arguments.
    lint           # Run Clippy with warnings treated as errors.
    run *args      # Run the debug executable with optional arguments.
    test           # Run all tests.
```

## Outputs

- `output/ot.json`, `output/rd.json`: card datasets sorted by ID.
- `output/ot.diff`, `output/rd.diff`: separate unified patches with one context line.
- `output/report.md`: change counts, added-card lists for OT/RD, image validation results, and warning/error details.
- `output/comparison.json`: the fixed baseline and change status consumed by the publishing workflow.

## Configuration and workflows

- Field definitions: [ygo-definitions](https://github.com/arshtyi/ygo-definitions).
- Field mappings: `config/{ot,rd}-field-mappings.json`.
- [CI](.github/workflows/ci.yml).
- [Publish](.github/workflows/publish-data.yml).
