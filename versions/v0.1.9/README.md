# v0.1.9 — background verification of working versions

A working version, not a release. `bash scripts/test-working-version.sh v0.1.8` tests the selected committed version without opening the app, using a separate build directory and test profile. Results are saved for the local dashboard. Automated tests do not verify a real account or native UI.
