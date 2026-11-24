# xdytube

<img src="assets/logo.png" alt="xdytube icon" width="96">

A personal, just-for-fun fork of [Stream Tube Desktop](https://github.com/Avaneesh-Inamdar/Stream-Tube-Desktop) by Avaneesh Inamdar.

## macOS on Apple Silicon

Download the ZIP from [GitHub Releases](https://github.com/dirasx/xdytube/releases), install libmpv with `brew install mpv`, and move `xdytube.app` to Applications. The build is ad hoc signed. If macOS blocks the first launch, use Privacy & Security to open it.

This fork includes a chronological subscription grid, an account Watch Later shortcut, a separate video fullscreen mode, an optional automatic miniplayer, corrected 4:3 thumbnails, and Yandex Browser support for connecting YouTube watch history. The sign-in flow uses a separate temporary browser profile.

The macOS build reuses the Stream Tube data directory so existing settings and playback history carry over.

## Source and license

Build on Apple Silicon with `bash packaging/macos/build-app.sh`. Upstream source and attribution are preserved under the [MIT License](LICENSE).

## Для разработчиков

[Передача проекта xdytube](docs/HANDOFF.md) — изменения форка, сборка и текущие ограничения.
