# Layers benchmark

Run `node benchmarks/layers.mjs`. The benchmark compiles its TypeScript entry points before timing; it does not run a test suite or build the application.

Fixture: 640 boxes in one boundary, 320 selected, 30 samples after warm-up. React server rendering uses a 280px initial viewport plus four overscan rows. Browser layout, paint, events and React update reconciliation are excluded.

Measured locally on 2026-10-29:

| Measurement | Before | After |
| --- | ---: | ---: |
| Layers SSR median | 54.257 ms | 2.306 ms |
| Rendered element rows | 640 | 14 |
| Markup bytes | 1,259,556 | 28,353 |
| Flatten 640 layers | — | 0.094 ms |
| Shared Properties values, 320 elements | 0.649 ms | 0.402 ms |

Layers baseline was captured on the original ElementLayers component before editing, using the same project, selection and SSR timing loop. Properties compares the former per-field map/commonValue calculations against the one-pass implementation in the same run. Timings vary with machine load; these are not measurements of the reported real-app stalls.

The benchmark also checks reverse sibling order, original insertion indices, nesting, collapse, search ancestors/descendants and shared/Mixed value equivalence. Native scrolling, keyboard focus, dragging and browser frame times require interactive verification; none was run for this change.
