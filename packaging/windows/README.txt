Tor V3 Vanity Address Generator @VERSION@ - Tor v3 .onion vanity address generator
https://github.com/Mxrider007/tor-v3-vanity-address-generator


QUICK START
-----------
1. Double-click  torv3-vanity-gui.exe
   If Windows says "Windows protected your PC", click "More info" and then
   "Run anyway". The program is not code-signed, which is why Windows asks.
2. Type what your address should start with, for example:  shop
3. Click "Start search".
4. When it is done, click "Show keys folder".

Nothing needs to be installed. A dedicated graphics card (NVIDIA, AMD or
Intel) is used automatically if one is found - no CUDA or other toolkit is
needed. Tick the graphics card box in the app to also use integrated
graphics.


WHAT YOU GET
------------
Each address is saved as a folder named after it, by default in your user
folder: C:\Users\<you>\Tor Vanity Keys. The folder contains:

  hostname               your new address (xxxxxxxx.onion)
  hs_ed25519_secret_key  the private key. KEEP IT SECRET AND BACK IT UP.
                         Anyone who has this file controls your address,
                         and if you lose it the address is gone for good.
  hs_ed25519_public_key  the public key
  authorized_clients     an empty folder that Tor expects

Keep the keys out of folders that OneDrive, Dropbox or Google Drive upload
(Documents and Desktop often are). Folders inside C:\Users\<you> can only be
read by you and administrators; folders elsewhere, such as C:\keys, are
often readable by every user of the computer.


HOW LONG DOES IT TAKE?
----------------------
Every extra character makes the search 32 times longer. Rough average times
on the CPU of a modern PC (a graphics card can be much faster):

  5 characters   seconds
  6 characters   under a minute
  7 characters   minutes to an hour
  8 characters   hours to a day
  9 characters   days to weeks

The app shows an estimate for your computer before you start.
Only letters a-z and digits 2-7 exist in onion addresses (no 0, 1, 8 or 9).
Use ? to mean "any character". Searching for several patterns at once
(separated by commas) costs nothing extra.


USING THE ADDRESS WITH TOR
--------------------------
Copy the whole folder to the computer that runs Tor (on Windows, a short
path without spaces is easiest, such as C:\tor\my_site) and add to its
torrc:

  HiddenServiceDir C:\tor\my_site
  HiddenServicePort 80 127.0.0.1:8080

then restart Tor. On Linux the folder is usually placed in /var/lib/tor/,
owned by the Tor user, with permissions 700. Full guide:
https://github.com/Mxrider007/tor-v3-vanity-address-generator#using-your-address-with-tor


COMMAND LINE
------------
torv3-vanity.exe is the command-line version. Open PowerShell in this folder
(in File Explorer type "powershell" into the address bar and press Enter)
and run, for example:

  .\torv3-vanity.exe shop
  .\torv3-vanity.exe shop blog -n 3 -o "$HOME\onion-keys"
  .\torv3-vanity.exe shop --estimate
  .\torv3-vanity.exe --help


LICENSE
-------
MIT License, see LICENSE.txt. Third-party software licenses are listed in
THIRD-PARTY-LICENSES.html.
