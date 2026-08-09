# ygo-cards

Yu-Gi-Oh! card data generator for OT and RD environments.

The tool downloads upstream YGOPro-compatible resources, normalizes card records, and writes sorted JSON outputs for downstream consumers.

## Outputs

- `output/ot.json`: normalized OT card data.
- `output/rd.json`: normalized RD card data.
- `output/report.md`: release-ready Markdown with an at-a-glance dataset summary, new cards since the current `latest` release, image validation, and grouped build diagnostics.
- `output/build.log`: numbered, structured warning and error records with aligned context, reasons, suggestions, and final severity totals.

## Field Definitions

Canonical data-field definitions are maintained in [arshtyi/ygo-definitions](https://github.com/arshtyi/ygo-definitions).

Raw database codes, bit flags, output values, and name/position mappings are maintained in `config/ot-field-mappings.json` and `config/rd-field-mappings.json`.

Source-resource, published-dataset, and card-image endpoints are maintained in `config/endpoints.json`.
