Kore Vanity @VERSION@ - Tor v3 .onion vanity address generator
https://github.com/Mxrider420z/kore-vanity


QUICK START
-----------
1. Double-click  kore-vanity-gui.exe
   If Windows says "Windows protected your PC", click "More info" and then
   "Run anyway". The program is not code-signed, which is why Windows asks.
2. Type what your address should start with, for example:  kore
3. Click "Start search".
4. When it is done, click "Show keys folder".

Nothing needs to be installed. A dedicated graphics card (NVIDIA, AMD or
Intel) is used automatically if one is found - no CUDA or other toolkit is
needed.


WHAT YOU GET
------------
Each address is saved as a folder named after it, by default in
Documents\Kore Vanity Keys. The folder contains:

  hostname               your new address (xxxxxxxx.onion)
  hs_ed25519_secret_key  the private key. KEEP IT SECRET AND BACK IT UP.
                         Anyone who has this file controls your address,
                         and if you lose it the address is gone for good.
  hs_ed25519_public_key  the public key
  authorized_clients     an empty folder that Tor expects


HOW LONG DOES IT TAKE?
----------------------
Every extra character makes the search 32 times longer. Rough times on a
modern desktop PC:

  5 characters   seconds
  6 characters   about a minute
  7 characters   under an hour
  8 characters   several hours to a day
  9 characters   weeks

The app shows an estimate for your computer before you start.
Only letters a-z and digits 2-7 exist in onion addresses (no 0, 1, 8 or 9).
Use ? to mean "any character". Searching for several patterns at once
(separated by commas) costs nothing extra.


USING THE ADDRESS WITH TOR
--------------------------
Copy the whole folder to the computer that runs Tor and add to its torrc:

  HiddenServiceDir C:\path\to\the\folder
  HiddenServicePort 80 127.0.0.1:8080

then restart Tor. On Linux the folder is usually placed in /var/lib/tor/,
owned by the Tor user, with permissions 700. Full guide:
https://github.com/Mxrider420z/kore-vanity#using-your-address-with-tor


COMMAND LINE
------------
kore-vanity.exe is the command-line version. Open PowerShell in this folder
(in File Explorer type "powershell" into the address bar and press Enter)
and run, for example:

  .\kore-vanity.exe kore
  .\kore-vanity.exe kore blog -n 3 -o C:\onion-keys
  .\kore-vanity.exe kore --estimate
  .\kore-vanity.exe --help


LICENSE
-------
MIT License, see LICENSE.txt. Third-party software licenses are listed in
THIRD-PARTY-LICENSES.html.
