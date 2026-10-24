#!/usr/bin/env bash
# Assemble a release archive in dist/ from already-built binaries.
#
#   packaging/package.sh <windows-x64|linux-x64|macos-universal> <tag>
#
# Expects THIRD-PARTY-LICENSES.html in the repository root (see about.toml).
# On Windows, run it from Git Bash; it uses 7-Zip if `7z` is on the PATH and
# PowerShell's Compress-Archive otherwise.
set -euo pipefail

platform=$1
tag=$2
version=${tag#v}
name="torv3-vanity-${tag}-${platform}"
root=$(pwd)
stage="$root/dist/stage/$name"

rm -rf "$root/dist"
mkdir -p "$stage"

# Plain-text files for Windows get CRLF line endings so Notepad shows them well.
crlf() { sed 's/\r*$/\r/' "$1" > "$2"; }
readme() { sed "s/@VERSION@/$version/g" "$1"; }

case "$platform" in
  windows-x64)
    cp target/release/torv3-vanity-gui.exe target/release/torv3-vanity.exe "$stage/"
    readme packaging/windows/README.txt > "$root/dist/README.tmp"
    crlf "$root/dist/README.tmp" "$stage/README.txt"
    rm "$root/dist/README.tmp"
    crlf LICENSE "$stage/LICENSE.txt"
    cp THIRD-PARTY-LICENSES.html "$stage/"
    if command -v 7z > /dev/null; then
      (cd "$root/dist/stage" && 7z a -tzip -mx=9 "$root/dist/$name.zip" "$name" > /dev/null)
    else
      (cd "$root/dist/stage" && powershell -NoProfile -Command \
        "Compress-Archive -Path '$name' -DestinationPath '$name.zip'" && mv "$name.zip" "$root/dist/")
    fi
    ;;
  linux-x64)
    # Release builds target glibc 2.28 with cargo-zigbuild (see release.yml).
    bin=target/x86_64-unknown-linux-gnu/release
    [ -x "$bin/torv3-vanity" ] || bin=target/release
    cp "$bin/torv3-vanity-gui" "$bin/torv3-vanity" "$stage/"
    readme packaging/linux/README.txt > "$stage/README.txt"
    cp LICENSE THIRD-PARTY-LICENSES.html "$stage/"
    cp gui/assets/icon.png "$stage/torv3-vanity.png"
    cp packaging/linux/torv3-vanity.desktop "$stage/"
    tar -C "$root/dist/stage" -czf "$root/dist/$name.tar.gz" "$name"
    ;;
  macos-universal)
    lipo -create -output "$stage/torv3-vanity" \
      target/aarch64-apple-darwin/release/torv3-vanity target/x86_64-apple-darwin/release/torv3-vanity
    app="$stage/Tor V3 Vanity.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    lipo -create -output "$app/Contents/MacOS/torv3-vanity-gui" \
      target/aarch64-apple-darwin/release/torv3-vanity-gui target/x86_64-apple-darwin/release/torv3-vanity-gui
    sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
    cp packaging/macos/AppIcon.icns "$app/Contents/Resources/"
    # Ad-hoc signatures: required for Apple Silicon to run the binaries at all.
    codesign --force --sign - "$stage/torv3-vanity"
    codesign --force --deep --sign - "$app"
    readme packaging/macos/README.txt > "$stage/README.txt"
    cp LICENSE THIRD-PARTY-LICENSES.html "$stage/"
    (cd "$root/dist/stage" && ditto -c -k --keepParent "$name" "$root/dist/$name.zip")
    ;;
  *)
    echo "unknown platform: $platform" >&2
    exit 1
    ;;
esac

rm -rf "$root/dist/stage"
ls -la "$root/dist"
