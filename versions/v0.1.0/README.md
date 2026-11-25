# Working version v0.1.0

Based on fork `0.7.2-alpha.xdy.1`, commit `c811140`. This starts xdytube's own version numbering.

## Changes

- macOS icon based on variant 4, commit `64efde0`.
- Browser selection before connecting YouTube watch history. Google Chrome and Yandex Browser are listed separately; duplicate entries are removed. Commit `1a6b94e`.
- Project workflow, status, changelog, and tasks moved to the `xdy-docsproduct` structure.

## Validation

- Compared the PNG icon with the original design and generated and inspected the `.icns` with `iconutil`.
- `cargo test --release --locked` on Apple Silicon: 37 passed, 20 ignored. The browser-selection test checks both entries, paths, and deduplication.
- At this stage, launching both browsers through the UI, synchronizing history with a real account, and the Finder and Dock icon remained to be checked manually.
