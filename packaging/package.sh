#!/usr/bin/env bash
# Assemble a release archive in dist/ from already-built binaries.
#
#   packaging/package.sh <windows-x64|linux-x64|macos-universal> <tag>
#
# Expects THIRD-PARTY-LICENSES.html in the repository root (see about.toml).
set -euo pipefail

platform=$1
tag=$2
version=${tag#v}
name="kore-vanity-${tag}-${platform}"
root=$(pwd)
stage="$root/dist/stage/$name"

rm -rf "$root/dist"
mkdir -p "$stage"

# Plain-text files for Windows get CRLF line endings so Notepad shows them well.
crlf() { sed 's/\r*$/\r/' "$1" > "$2"; }
readme() { sed "s/@VERSION@/$version/g" "$1"; }

case "$platform" in
  windows-x64)
    cp target/release/kore-vanity-gui.exe target/release/kore-vanity.exe "$stage/"
    readme packaging/windows/README.txt > "$root/dist/README.tmp"
    crlf "$root/dist/README.tmp" "$stage/README.txt"
    rm "$root/dist/README.tmp"
    crlf LICENSE "$stage/LICENSE.txt"
    cp THIRD-PARTY-LICENSES.html "$stage/"
    (cd "$root/dist/stage" && 7z a -tzip -mx=9 "$root/dist/$name.zip" "$name" > /dev/null)
    ;;
  linux-x64)
    cp target/release/kore-vanity-gui target/release/kore-vanity "$stage/"
    readme packaging/linux/README.txt > "$stage/README.txt"
    cp LICENSE THIRD-PARTY-LICENSES.html "$stage/"
    cp gui/assets/icon.png "$stage/kore-vanity.png"
    cp packaging/linux/kore-vanity.desktop "$stage/"
    tar -C "$root/dist/stage" -czf "$root/dist/$name.tar.gz" "$name"
    ;;
  macos-universal)
    lipo -create -output "$stage/kore-vanity" \
      target/aarch64-apple-darwin/release/kore-vanity target/x86_64-apple-darwin/release/kore-vanity
    app="$stage/Kore Vanity.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    lipo -create -output "$app/Contents/MacOS/kore-vanity-gui" \
      target/aarch64-apple-darwin/release/kore-vanity-gui target/x86_64-apple-darwin/release/kore-vanity-gui
    sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
    cp packaging/macos/AppIcon.icns "$app/Contents/Resources/"
    # Ad-hoc signatures: required for Apple Silicon to run the binaries at all.
    codesign --force --sign - "$stage/kore-vanity"
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
