# Working version v0.1.2 — profile and Keychain

Based on [`v0.1.1`](../v0.1.1/README.md). xdytube uses its own profile directory and instance file. On first launch it migrates an agreed snapshot of the previous SQLite database, including history, subscriptions, playlists, progress, and settings. The original profile remains available for rollback.

Both YouTube sign-ins must be reconnected. New credentials are stored in macOS Keychain and separated by profile. Diagnostic exports contain only the version, platform, and selected playback settings.

Tests covered WAL data, restart without overwriting new progress, preservation of the old profile, and rollback after migration errors. Separate credential deletion was tested with a test store. Synthetic Keychain credentials were written, read from a new process, and deleted on macOS. Real-account sign-in and Keychain behavior during a signed-app update required separate manual verification.
