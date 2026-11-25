#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/../.."

if [[ $(uname -s) != Darwin || $(uname -m) != arm64 ]]; then
    echo "Build the app on macOS Apple Silicon with Xcode installed." >&2
    exit 1
fi

cargo build --release --locked >&2
metadata=$(cargo metadata --format-version 1 --no-deps --locked)
version=$(printf '%s' "$metadata" | plutil -extract packages.0.version raw -o - -)
target_dir=$(printf '%s' "$metadata" | plutil -extract target_directory raw -o - -)
binary="$target_dir/release/streamtube"
minimum_system=$(xcrun vtool -show-build "$binary" | awk '$1 == "minos" { print $2 }')
[[ -n $minimum_system ]]

output_root="${1:-$PWD/dist/macos}"
mkdir -p "$output_root"
output_root=$(cd "$output_root" && pwd -P)
output=$(mktemp -d "$output_root/xdytube-$version-$(uname -m).XXXXXXXX")
app="$output/xdytube.app"
iconset="$output/xdytube.iconset"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$iconset"
install -m 755 "$binary" "$app/Contents/MacOS/xdytube"
install -m 644 LICENSE "$app/Contents/Resources/LICENSE"
for size in 16 32 128 256; do
    sips -z "$size" "$size" assets/macos-logo.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" assets/macos-logo.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/xdytube.icns"
rm -r "$iconset"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>xdytube</string>
    <key>CFBundleIconFile</key>
    <string>xdytube.icns</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.dirasx.xdytube</string>
    <key>CFBundleDisplayName</key>
    <string>xdytube</string>
    <key>CFBundleName</key>
    <string>xdytube</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${version%%-*}</string>
    <key>CFBundleVersion</key>
    <string>${version%%-*}</string>
    <key>LSMinimumSystemVersion</key>
    <string>$minimum_system</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.entertainment</string>
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>CFBundleURLName</key>
            <string>io.github.dirasx.xdytube.link</string>
            <key>CFBundleURLSchemes</key>
            <array><string>xdytube</string></array>
        </dict>
    </array>
</dict>
</plist>
EOF

plutil -lint "$app/Contents/Info.plist" >&2
codesign --force --sign - "$app" >&2
codesign --verify --strict "$app"
printf '%s\n' "$app"
