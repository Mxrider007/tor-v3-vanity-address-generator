Tor V3 Vanity Address Generator @VERSION@ - Tor v3 .onion vanity address generator
https://github.com/Mxrider007/tor-v3-vanity-address-generator

QUICK START
  Open "Tor V3 Vanity.app" (you may drag it to Applications first).

  The app is not notarized by Apple, so the first time macOS will refuse to
  open it. Either right-click (Control-click) the app, choose Open and
  confirm, or allow it in System Settings > Privacy & Security > "Open
  Anyway". Alternatively run in Terminal:
      xattr -dr com.apple.quarantine "Tor V3 Vanity.app" torv3-vanity

COMMAND LINE
  ./torv3-vanity shop           address starting with "shop"
  ./torv3-vanity --help         all options

Runs natively on Apple Silicon and Intel Macs (macOS 11 or newer) and uses
the GPU through Metal.

WHAT YOU GET
Each address is saved as a folder named after it containing hostname,
hs_ed25519_public_key, hs_ed25519_secret_key and authorized_clients/.
Keep hs_ed25519_secret_key secret and backed up: whoever has it controls the
address. See the project page for how to use it with Tor.

Every extra character makes the search 32 times longer. Letters a-z and
digits 2-7 only; ? matches any character.

MIT License (see LICENSE); third-party licenses in THIRD-PARTY-LICENSES.html.
