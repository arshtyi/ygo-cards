# ygo-cards

Yu-Gi-Oh! card data generator for OT and RD environments.

The tool downloads upstream YGOPro-compatible resources, normalizes card records, and writes sorted JSON outputs for downstream consumers.

## Outputs

- `output/ot.json`: normalized OT card data.
- `output/rd.json`: normalized RD card data.
- `output/report.md`: release-ready Markdown with an at-a-glance dataset summary, new cards since the current `latest` release, image validation, forbidden-list statistics, and grouped build diagnostics.
- `output/build.log`: numbered, structured warning and error records with aligned context, reasons, suggestions, and final severity totals.

Normal progress and dataset summaries are written to stdout. Fatal failures are written to stderr with their complete error chain and the diagnostics path.

## Usage

```console
Generate normalized Yu-Gi-Oh! card data for OT and RD environments

Usage: ygo-cards [OPTIONS]

Options:
      --refresh-resources
          Download fresh upstream resources before generating card data
      --check-images
          Check primary and alias images; failed checks use image 0 by default
      --skip-image-failures
          Skip cards whose primary and alias images both fail (requires --check-images)
      --include-aliases-in-lf-statistics
          Include cards with alias != 0 in forbidden-list statistics
  -h, --help
          Print help
  -V, --version
          Print version
```

## Field Definitions

Canonical data-field definitions are maintained in [arshtyi/ygo-definitions](https://github.com/arshtyi/ygo-definitions).

Bitmask mappings for attributes, types, races, link markers, and RD maximum markers are maintained in `config/ot-masks.json` and `config/rd-masks.json`.

Remote resource, latest-release, and image URLs are maintained in `config/urls.json`.

## Releases

The scheduled workflow compares the generated `ot.json` and `rd.json` with the current latest release. It skips publication when both datasets are unchanged; otherwise, it publishes the next `0.0.N` version (starting at `0.0.1`) and marks that release as latest. The release body uses `output/report.md` directly, with release and commit metadata added by the workflow; diagnostics are already grouped and formatted in the report instead of being appended as a duplicate raw log.
