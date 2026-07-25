# ygo-cards

Yu-Gi-Oh! card data generator for OT and RD environments.

The tool downloads upstream YGOPro-compatible resources, normalizes card records, and writes sorted JSON outputs for downstream consumers.

## Outputs

- `output/ot.json`: normalized OT card data.
- `output/rd.json`: normalized RD card data.
- `output/report.md`: build summary, forbidden-list counts, skipped-card counts, image-check results when enabled, and new cards compared with the current `latest` release when available.

## Usage

```console
Generate normalized Yu-Gi-Oh! card data for OT and RD environments

Usage: ygo-cards [OPTIONS]

Options:
      --refresh-resources    Download fresh upstream resources before generating card data
      --check-images         Check primary and alias images; failed checks use image 0 by default
      --skip-image-failures  Skip cards whose primary and alias images both fail (requires --check-images)
  -h, --help                 Print help
  -V, --version              Print version
```

## Field Definitions

Canonical data-field definitions are maintained in [arshtyi/ygo-definitions](https://github.com/arshtyi/ygo-definitions).

Bitmask mappings for attributes, types, races, link markers, and RD maximum markers are maintained in `config/ot-masks.json` and `config/rd-masks.json`.

Remote resource, latest-release, and image URLs are maintained in `config/urls.json`.
