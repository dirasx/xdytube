# xdytube

<img src="assets/macos-logo.png" alt="xdytube icon" width="96">

A personal, just-for-fun fork of [Stream Tube Desktop](https://github.com/Avaneesh-Inamdar/Stream-Tube-Desktop) by Avaneesh Inamdar.

## macOS on Apple Silicon

Download the ZIP from [GitHub Releases](https://github.com/dirasx/xdytube/releases), install libmpv with `brew install mpv`, and move `xdytube.app` to Applications. The build is ad hoc signed. If macOS blocks the first launch, use Privacy & Security to open it.

This fork includes a chronological subscription grid, an account Watch Later shortcut, a separate video fullscreen mode, an optional automatic miniplayer, and corrected 4:3 thumbnails. The current source lets you choose an installed browser, including Chrome or Yandex Browser, when connecting YouTube watch history. The sign-in flow uses a separate temporary browser profile.

Starting with R0.1, xdytube uses its own profile and migrates local settings and playback history on first start. It stores new YouTube sign-ins in macOS Keychain. Reconnect both YouTube sign-ins after updating from the alpha.

## Source and license

Build on Apple Silicon with `bash packaging/macos/build-app.sh`. Upstream source and attribution are preserved under the [MIT License](LICENSE).

## Для разработчиков

[Сборка и участие](CONTRIBUTING.md) · [История изменений](CHANGELOG.md)
