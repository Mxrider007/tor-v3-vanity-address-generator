#!/usr/bin/env bash
# Print the GitHub release notes for a tag: download instructions followed by
# the matching CHANGELOG.md section.   packaging/release-notes.sh v1.0.0
set -euo pipefail

tag=$1
version=${tag#v}
section=$(awk -v v="$version" '
  /^## \[/ { if (found) exit; if (index($0, "[" v "]")) { found = 1; next } }
  found { print }
' CHANGELOG.md)
if [ -z "$section" ]; then
  echo "CHANGELOG.md has no section for $version" >&2
  exit 1
fi

cat <<NOTES
## Download

| System | File |
|---|---|
| **Windows 10/11** (64-bit) | [\`kore-vanity-$tag-windows-x64.zip\`](https://github.com/Mxrider420z/kore-vanity/releases/download/$tag/kore-vanity-$tag-windows-x64.zip) |
| **macOS 11+** (Apple Silicon and Intel) | [\`kore-vanity-$tag-macos-universal.zip\`](https://github.com/Mxrider420z/kore-vanity/releases/download/$tag/kore-vanity-$tag-macos-universal.zip) |
| **Linux** (x86-64) | [\`kore-vanity-$tag-linux-x64.tar.gz\`](https://github.com/Mxrider420z/kore-vanity/releases/download/$tag/kore-vanity-$tag-linux-x64.tar.gz) |

**Windows:** download the zip, right-click it and choose **Extract All**, then double-click **kore-vanity-gui.exe**.
If Windows shows *"Windows protected your PC"*, click **More info → Run anyway** (the program is not code-signed).
No installation, drivers or toolkits are needed; the graphics card is used automatically if it helps.

Check your download against \`SHA256SUMS.txt\` (\`Get-FileHash <file>\` in PowerShell, \`sha256sum <file>\` on Linux/macOS).

$section
NOTES
