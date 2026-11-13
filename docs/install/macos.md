# Linkwise Modem Preview for macOS

Linkwise provides a two-way software modem, station diagnostics, and local
modem simulations. See Station Setup for audio routing and transmit settings.

## Install or update

1. In Apple menu → About This Mac, check the chip or processor. Choose the
   `macos-arm64.pkg` for Apple silicon or `macos-x86_64.pkg` for Intel.
2. Open the package in Installer and follow the prompts. Installation
   requires an administrator account. For an update, run the newer package
   the same way. Keep your station configuration in your home directory.
3. Open Terminal and run:

   ```sh
   /usr/local/bin/linkwise-modemd --version
   /usr/local/bin/linkwise-modemd --simulate
   ```

The simulation runs locally without a radio. Development packages can be
unsigned. If macOS rejects the package, ask the distributor for a signed
and notarized build.

## Set up your station

Run `man 8 linkwise-modemd` for the command reference and
`man 5 linkwise-station` for the station configuration format.

Open `/usr/local/share/doc/linkwise-modem/Station-setup.md` for instructions
on selecting devices and creating `station.json`. If capture is denied, check
System Settings → Privacy & Security → Microphone for your terminal app.

## Run the modem login agent

On macOS 13 or later, save the station configuration under
`~/Library/Application Support/Linkwise/station.json`, then run:

```sh
"/Applications/Linkwise Station.app/Contents/MacOS/LinkwiseStation" enable
```

Grant microphone access and approve the background item in System Settings
if prompted. Use the same command with `status` to see registration state
or `disable` to unregister the agent before uninstalling.

## Uninstall

Quit any running Linkwise commands, then remove the installed files and
package receipt:

```sh
sudo rm -f /usr/local/bin/linkwise /usr/local/bin/linkwise-modemd \
  /usr/local/share/doc/linkwise-modem/Install.md \
  /usr/local/share/doc/linkwise-modem/LICENSE \
  /usr/local/share/doc/linkwise-modem/THIRD-PARTY-NOTICES.txt \
  /usr/local/share/doc/linkwise-modem/Station-setup.md \
  /usr/local/share/man/man1/linkwise.1 \
  /usr/local/share/man/man8/linkwise-modemd.8 \
  /usr/local/share/man/man5/linkwise-station.5
sudo rm -rf "/Applications/Linkwise Station.app"
sudo pkgutil --forget io.github.8r4n.linkwise.modem
```

Station files saved in your home directory remain available.
