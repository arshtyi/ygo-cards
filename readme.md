# ygo-cards

Yu-Gi-Oh! card data generator for OT and RD environments.

The tool downloads upstream YGOPro-compatible resources, normalizes card records, and writes sorted JSON outputs for downstream consumers.

## Outputs

- `output/ot.json`: normalized OT card data.
- `output/rd.json`: normalized RD card data.
- `output/report.md`: build summary, forbidden-list counts, skipped-card counts, image-check results when enabled, and new cards compared with the current `latest` release when available.

## Usage

Generate JSON from local assets, downloading missing assets if needed:

```powershell
cargo run
```

Refresh upstream resources before generating:

```powershell
cargo run -- --refresh-resources
```

Validate card image availability while generating:

```powershell
cargo run -- --check-images
```

Run tests:

```powershell
cargo test
```

## Field Definitions

Canonical data-field definitions are maintained in [arshtyi/ygo-definations](https://github.com/arshtyi/ygo-definations).

Bitmask mappings for attributes, types, races, link markers, and RD maximum markers are maintained in `config/ot-masks.json` and `config/rd-masks.json`.

Remote resource, latest-release, and image URLs are maintained in `config/urls.json`.

## Automation

GitHub Actions publishes the generated JSON files to the `latest` release every Monday and Friday at 22:00 Beijing time(UTC+8).

Release assets:

- `ot.json`
- `rd.json`

Release notes are generated from `output/report.md`.
