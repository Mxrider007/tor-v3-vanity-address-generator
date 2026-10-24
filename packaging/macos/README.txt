Tor V3 Vanity Address Generator @VERSION@ - Tor v3 .onion vanity address generator
https://github.com/Mxrider007/tor-v3-vanity-address-generator

QUICK START
  Open "Tor V3 Vanity.app" (you may drag it to Applications first).

  The app is not notarized by Apple, so the first time macOS will refuse to
  open it. Try to open it once, then go to System Settings > Privacy &
  Security, scroll down and click "Open Anyway" (on macOS 11-14 you can
  instead Control-click the app and choose Open). Alternatively run this in
  Terminal, in the folder you unzipped:
      xattr -dr com.apple.quarantine "Tor V3 Vanity.app" torv3-vanity

COMMAND LINE
  ./torv3-vanity shop           address starting with "shop"
  ./torv3-vanity --help         all options

Runs natively on Apple Silicon and Intel Macs (macOS 11 or newer). The GPU
is used through Metal: by default on Apple Silicon and dedicated graphics
cards, and on Intel graphics if you tick the box in the app (or use
--gpu on).

WHAT YOU GET
Each address is saved as a folder named after it containing hostname,
hs_ed25519_public_key, hs_ed25519_secret_key and authorized_clients/.
Keep hs_ed25519_secret_key secret and backed up: whoever has it controls the
address. The app saves keys in ~/Tor Vanity Keys by default. Avoid Desktop
and Documents if they are synced to iCloud Drive. See the project page for
how to use the keys with Tor.

Every extra character makes the search 32 times longer. Letters a-z and
digits 2-7 only; ? matches any character (quote it in the shell: "sh?p").

MIT License (see LICENSE); third-party licenses in THIRD-PARTY-LICENSES.html.
