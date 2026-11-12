# Linkwise Modem Preview for Fedora

Linkwise provides command-line station checks and modem simulations. Live
Winlink message transfer and radio PTT are still under development.

## Install or update

Run `uname -m` to find your architecture. In the directory where you
downloaded the matching package, run the following command, substituting
the downloaded filename:

```sh
sudo dnf install ./linkwise-modem-0.1.0-1.aarch64.rpm
linkwise-modemd --version
linkwise-modemd --simulate
```

DNF installs the required system libraries. Use `sudo dnf upgrade ./FILE.rpm`
with the newer file to update. The simulation runs locally without a radio.

## Set up your station

Open `/usr/share/doc/linkwise-modem/Station-setup.md` for instructions on
selecting devices and creating `station.json`. Run audio checks as your
desktop user so Linkwise can access the current audio session.

## Uninstall

Close running Linkwise commands, then run `sudo dnf remove linkwise-modem`.
Station files saved in your home directory remain available.
