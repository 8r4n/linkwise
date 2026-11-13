# Linkwise Modem Preview for Ubuntu

Linkwise provides a two-way software modem, station diagnostics, and local
modem simulations. See Station Setup for audio routing and transmit settings.

## Install or update

Run `dpkg --print-architecture` to find your architecture. In the directory
where you downloaded the matching package, run the following command,
substituting the downloaded filename:

```sh
sudo apt install ./linkwise-modem_0.1.0_arm64.deb
linkwise-modemd --version
linkwise-modemd --simulate
```

APT installs the required system libraries. Run the same installation command
with the newer file to update. The simulation runs locally without a radio.

## Set up your station

Run `man 8 linkwise-modemd` for the command reference and
`man 5 linkwise-station` for the station configuration format.

Open `/usr/share/doc/linkwise-modem/Station-setup.md` for instructions on
selecting devices and creating `station.json`. Run audio checks as your
desktop user so Linkwise can access the current audio session.

## Run the modem service

For a desktop station, save the configuration to
`~/.config/linkwise/station.json`, then run:

```sh
systemctl --user enable --now linkwise-modem.service
journalctl --user -u linkwise-modem.service
```

For a dedicated station, put administrator-owned configuration and profile
files under `/etc/linkwise`. The service needs read access to these files
and direct ALSA access to the configured input and output. Use only one service for
a given device and port pair.

```sh
sudo systemctl enable --now linkwise-modem.service
sudo journalctl -u linkwise-modem.service
```

With `bandwidth` configured, the modem opens its command and data ports on
IPv4 loopback. Without `bandwidth`, the command port serves receive diagnostics
as JSON. Read it with `nc 127.0.0.1 8300`.

## Uninstall

Close running Linkwise commands, then run `sudo apt remove linkwise-modem`.
Station files saved in your home directory remain available.
