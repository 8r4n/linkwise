# Linkwise Modem Preview for macOS

Linkwise provides command-line station checks and modem simulations. Live
Winlink message transfer and radio PTT are still under development.

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

Open `/usr/local/share/doc/linkwise-modem/Station-setup.md` for instructions
on selecting devices and creating `station.json`. If capture is denied, check
System Settings → Privacy & Security → Microphone for your terminal app.

## Uninstall

Quit any running Linkwise commands, then remove the installed files and
package receipt:

```sh
sudo rm -f /usr/local/bin/linkwise /usr/local/bin/linkwise-modemd \
  /usr/local/share/doc/linkwise-modem/Install.md \
  /usr/local/share/doc/linkwise-modem/Station-setup.md
sudo pkgutil --forget io.github.8r4n.linkwise.modem
```

Station files saved in your home directory remain available.
