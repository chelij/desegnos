# Frozen generation migration fixture

`schema6-format9-selection.desegnos` and `schema6-format9-selection.json` were produced offline by the actual native `GenerationService` and `ProjectFile` implementations at WebMaker commit `06613a1` (schema 6, requirements format 9), before stable element IDs were introduced. The JSON includes the original project and Markdown, the persisted Prepared record, and the persisted Ready record after one deterministic local provider response. No credential or network request was used.

The request changes one existing heading and preserves an adjacent paragraph. Its positional selector and source fingerprint are genuine old-format values. The Ready result was validated by that pre-migration implementation before export. These are immutable compatibility artifacts; do not regenerate them with the current schema or relabel a format-10 packet as format 9.

SHA-256:

- Archive: `f82d969576c6e96e9a82f2e641a707609998e5723cf58d9e4a50b7b5891e1956`
- JSON: `d2fe25c4e17dc4a1bdbbb1306d03dda9178f028f33caa069fd102820a1b76b0e`
