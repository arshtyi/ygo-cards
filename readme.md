# ygo-cards

Yu-Gi-Oh! card data generator for OT and RD environments.

The tool downloads upstream YGOPro-compatible resources, normalizes card records, and writes sorted JSON outputs for downstream consumers.

## Outputs

- `output/ot.json`: normalized OT card data.
- `output/rd.json`: normalized RD card data.
- `output/report.md`: release-ready Markdown with an at-a-glance dataset summary, new cards since the previous release, image validation, and grouped build diagnostics.
- `output/build.log`: numbered, structured warning and error records with aligned context, reasons, suggestions, and final severity totals.

## Field Definitions

Canonical data-field definitions are maintained in [arshtyi/ygo-definitions](https://github.com/arshtyi/ygo-definitions).

Raw database codes, bit flags, output values, and name/position mappings are maintained in `config/ot-field-mappings.json` and `config/rd-field-mappings.json`.

Source-resource, published-dataset, and card-image endpoints are maintained in `config/endpoints.json`.

## Workflows

- [CI](.github/workflows/ci.yml) runs formatting checks, Clippy, and tests on pull requests and pushes to `main`.
- [Publish card data](.github/workflows/publish-data.yml) runs manually or on Mondays and Fridays at 10:00 UTC. It generates data from `main` with image validation, skips publication when the datasets are unchanged, and otherwise publishes the next `0.0.x` release with `output/report.md` as its release notes. Each release uses an annotated tag with the message `chore(release): version bump to <version>`.
