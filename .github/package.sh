#!/usr/bin/env bash
# Packages the release build for this runner's platform as $ASSET.exe or $ASSET.tar.gz.
# VERSION is the release version without the leading "v".
set -euo pipefail

case "$RUNNER_OS" in
  Windows)
    mv target/release/osu-tablet-optimizer.exe "$ASSET.exe"
    ;;
  macOS)
    # A bundle, so Finder and the Dock show the icon. Signed ad hoc: Apple silicon only runs
    # signed code, and the signature has to cover the bundle's Info.plist too.
    app="osu! Tablet Optimizer.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp target/release/osu-tablet-optimizer "$app/Contents/MacOS/"
    cp assets/icon/AppIcon.icns "$app/Contents/Resources/"
    # Bundle versions are numbers only, so a prerelease suffix like -rc1 is dropped.
    sed "s/VERSION/${VERSION%%-*}/g" assets/macos/Info.plist > "$app/Contents/Info.plist"
    codesign --force --sign - "$app"
    tar -czf "$ASSET.tar.gz" "$app"
    ;;
  Linux)
    # Linux executables carry no icon; launchers take it from the .desktop file.
    tar -czf "$ASSET.tar.gz" -C target/release osu-tablet-optimizer \
      -C "$PWD/assets/linux" osu-tablet-optimizer.desktop osu-tablet-optimizer.png
    ;;
esac
