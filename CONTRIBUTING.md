# Contributing to xdytube

Thanks for wanting to help. Bug fixes, small improvements and translations are
always welcome. For anything bigger, please open an issue first so we can agree
on the approach before you spend time on it.

## Building

Build on a Mac with Apple Silicon. Install Homebrew libmpv with `brew install mpv`. CI uses Rust `1.100.0`.

```sh
cargo build --release --locked
cargo test --release --locked
```

See the [project workflow](WORKFLOW.md) for working task numbers and release candidates.

## Making a change

- **One change per pull request.** A bug fix and an unrelated feature are two PRs.
  Small PRs get reviewed and merged much faster.
- **Run the app and try your change.** Builds and tests passing isn't enough for
  UI or playback changes.
- **Include screenshots** (or a short clip) for anything you can see.
- **Follow the existing style.** Run `cargo fmt`, keep comments short and about
  *why* rather than *what*, and reuse the helpers in `src/ui/widgets.rs` and
  `src/ui/mod.rs` instead of adding new ones that do the same thing.
- **Avoid new dependencies** unless there's no reasonable alternative. If you add
  one, say why in the PR.
- **Keep `Cargo.lock` in the PR** if you changed dependencies; CI builds with `--locked`.

## Things that won't be merged

- Telemetry, analytics, crash reporting or anything else that sends data
  somewhere new
- Code that's minified, obfuscated or generated without being readable
- Changes to sign-in, cookies or watch-history reporting that send account data
  anywhere other than YouTube
- Large rewrites or reformatting of code you aren't otherwise changing

## Pull request checks

Pull requests are built and tested on macOS Apple Silicon. The manually dispatched release-candidate workflow produces an ad hoc-signed ZIP and its SHA-256 checksum.

## Reporting bugs

Open an issue with the app version (*Settings → About*), your OS, what you did,
what happened and what you expected. For playback problems, the video link and
the *Stats for nerds* panel (*Shift+I*) help a lot.

## License

By contributing, you agree that your contribution is licensed under the
project's [MIT License](LICENSE).
