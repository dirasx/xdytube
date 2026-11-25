# Working version v0.1.1

Based on [`v0.1.0`](../v0.1.0/README.md) and the published [`v0.0.1-alpha`](../../release-notes/R0.0.1-alpha.md).

## Changes

- A suitable saved position is applied when a video opens. The `Resume where you left off` toggle was removed. An explicit timestamp in a link takes priority.
- Closing a video while it loads preserves previous progress. A fallback stream uses the position requested when opening the video.

## Validation

- Before the fix, a regression test reproduced progress changing from `120 / 600` seconds to `0 / 0` when closing a loading video.
- `cargo test --release --locked` on Apple Silicon: 40 passed, 20 ignored.
- Opening from a card with real YouTube history and resuming after restart still required manual checks at this stage.
