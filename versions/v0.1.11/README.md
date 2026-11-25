# v0.1.11 — installing updates

A working version, not a release. The app checks published stable GitHub releases at startup and offers to install a compatible Apple Silicon build in settings. Before replacement, it checks the GitHub API SHA-256, archive structure, package signature integrity, identifier, version, and architecture. The previous app is retained for recovery; the user profile is not changed.

The app is ad hoc signed: this verifies package integrity but is not an independent developer signature. Source verification relies on HTTPS and GitHub metadata. A real installed-app update requires verification after a subsequent release.
