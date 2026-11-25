# Working version v0.1.3 — macOS Apple Silicon

Based on [`v0.1.2`](../v0.1.2/README.md). Removed project-specific Windows and Linux branches, platform resources, installers, and the local Windows GPUI patch. Regular CI checks macOS arm64 builds and tests only. A transitive GPUI dependency for Windows remains in the lockfile but is not built for Apple Silicon.

The `macOS release candidate` workflow is triggered explicitly and produces an ad hoc-signed ZIP and `SHA256SUMS.txt`; publication is a separate step. At this stage, the app opened the then-private GitHub releases page for manual updates.

`cargo check --release --locked` passed. A temporary ZIP, SHA-256, arm64 architecture, extracted signature, clean-profile launch, and handoff to an existing instance were checked. The minimum macOS version and cold-start browser links still required verification.
