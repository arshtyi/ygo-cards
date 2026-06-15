# Data Generation Notes

## Scope

This repository generates normalized card data for two environments:

- OT: OCG/TCG card pool, forbidden lists, and text resources.
- RD: Rush Duel card pool, forbidden list, and Rush-specific field handling.

The generated JSON files are arrays. Each array item is one card object. Object keys are sorted alphabetically.

## Resource Flow

1. Resource files are stored under `assets/`.
2. The generator reads SQLite card databases and text/config resources.
3. Invalid cards are skipped with explicit diagnostics.
4. JSON is written under `output/`.
5. A Markdown build report is written to `output/report.md`.

Use `--refresh-resources` when a build should force-download the upstream resources.

## Images

By default, `image` is set to the card ID without network checks.

With `--check-images`, the generator checks cropped card images from YGOPRODeck:

1. Try the card ID.
2. If unavailable and `alias > 0`, try the alias ID.
3. If both fail, set `image` to `0`.

Failed image cards are printed and listed in `output/report.md`.

## Forbidden Lists

OT outputs `lf` as `[ocg, tcg]`.

RD outputs `lf` as a single integer.

Values:

| Value | Meaning |
| ---: | --- |
| 0 | Forbidden |
| 1 | Limited |
| 2 | Semi-limited |
| 3 | Unlimited |

Alias IDs are considered when matching forbidden-list entries.

## Field Definitions

The authoritative field-definition reference lives in [arshtyi/ygo-definations](https://github.com/arshtyi/ygo-definations).

## Scheduled Publishing

The GitHub workflow `.github/workflows/publish-data.yml` runs:

- Monday 22:00 Beijing time.
- Friday 22:00 Beijing time.
- Manually via `workflow_dispatch`.

It refreshes resources, regenerates JSON, and overwrites the `latest` release with:

- `ot.json`
- `rd.json`

The release notes are generated from `output/report.md`.
