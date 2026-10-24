Tor V3 Vanity Address Generator @VERSION@ - Tor v3 .onion vanity address generator
https://github.com/Mxrider007/tor-v3-vanity-address-generator

QUICK START
  ./torv3-vanity-gui            graphical app
  ./torv3-vanity shop           command line: address starting with "shop"
  ./torv3-vanity --help         all options

Requirements: 64-bit Linux with glibc 2.28 or newer (RHEL/Rocky 8+,
Debian 10+, Ubuntu 18.10+). The app needs an X11 or Wayland desktop with
libxkbcommon (on X11 also libxkbcommon-x11) and an OpenGL or Vulkan driver.
The GPU is used for the search through Vulkan when a dedicated graphics card
is present (install your distribution's Vulkan driver, e.g.
mesa-vulkan-drivers or the NVIDIA driver); otherwise the CPU is used.

To add the app to your applications menu:
  install -Dm755 torv3-vanity-gui ~/.local/bin/torv3-vanity-gui
  install -Dm644 torv3-vanity.png ~/.local/share/icons/hicolor/256x256/apps/torv3-vanity.png
  install -Dm644 torv3-vanity.desktop ~/.local/share/applications/torv3-vanity.desktop

WHAT YOU GET
Each address is saved as a folder named after it containing hostname,
hs_ed25519_public_key, hs_ed25519_secret_key and authorized_clients/.
Keep hs_ed25519_secret_key secret and backed up: whoever has it controls the
address. The app saves keys in ~/Tor Vanity Keys by default; the command line
saves them in the current folder unless you pass -o DIR.

USING THE ADDRESS WITH TOR
  sudo cp -r ./keys/xxxx...xxxx /var/lib/tor/my_site
  sudo chown -R debian-tor:debian-tor /var/lib/tor/my_site   # Tor's user
  sudo chmod 700 /var/lib/tor/my_site
Then add to /etc/tor/torrc:
  HiddenServiceDir /var/lib/tor/my_site/
  HiddenServicePort 80 127.0.0.1:8080
and restart Tor (sudo systemctl restart tor).

Every extra character makes the search 32 times longer. Letters a-z and
digits 2-7 only; ? matches any character (quote it in the shell: "sh?p").

MIT License (see LICENSE); third-party licenses in THIRD-PARTY-LICENSES.html.
