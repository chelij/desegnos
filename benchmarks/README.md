# Canvas edit performance

Run the small frontend probe with `node benchmarks/canvas.mjs`. It compiles the actual
TypeScript functions using the existing test helper, excludes compilation from timings,
prints the median of 20 warm samples, and removes its temporary compilation directory.
It also checks selected-ancestor suppression, inherited outlines, camera projection and
multi-element drop behavior. DOM fixtures count queries; they do **not** measure browser
layout, React rendering or painting.

Native edit correctness is covered by `crates/desegnos-core/tests/hug_synchronization.rs`.
The native measurements below are historical observations, not a regression benchmark.

Local measurements on 2026-10-28 (milliseconds):

| Operation | Before | After |
| --- | ---: | ---: |
| Selection entries, 80 selected | 1.317 | 0.135 |
| Selection entries, 160 selected | 3.539 | 0.139 |
| Selection entries, 320 selected | 12.874 | 0.384 |
| Selection entries, 640 selected | 44.239 | 0.238 |
| Native move 1 of 640 | 0.637 | 0.244 |
| Native move all 640 | 3.440 | 2.646 |
| Native insert 320 into 640 | 1.608 | 0.353 |
| Native serialize 640 | 0.588 | 0.575 |

Cold canvas views varied around 4–9 ms; cached views were under 0.001 ms. Camera-only
projection of 640 already measured selection rectangles took 0.015 ms. These are local
microbenchmarks, not end-to-end timings of the reported software-rendered desktop session.

Selection processing previously rebuilt each element's parent path for every other
selected element. A set of selected ids now lets each path be checked once, and the
selection bar reuses those entries across camera changes. Selection measurement previously
queried the complete DOM separately for each selected id: **640 queries → 1**. Pan/zoom now
projects those measurements without DOM reads, remeasuring when geometry, fonts, surface
size, selection or boundary mounting changes.

Drop handling previously repeated the same box hit test for every moved layer:
**640 queries → 1** in the all-selected fixture. The result is now cached within the drop.
Rust previously resolved/cloned/measured every page and linked size twice, even when no
axis used Hug. Fixed-only projects now skip that pass and its redundant second validation;
projects with Hug still synchronize and validate after measurement.

Insertion already uses a single vector splice and one transaction per destination group.
Native movement still includes repeated vector/id lookups, but measured about 3 ms at 640
layers. The full serialized fixture is 488,317 bytes and serialization takes about 0.6 ms;
the IPC contract and snapshot ownership remain unchanged. Transport/decode time, real
WebKit layout/paint and end-to-end software-rendered gestures remain unmeasured here.
Properties, Layers and keyboard/command handling were left untouched.
