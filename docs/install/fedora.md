# Linkwise Modem Preview for Fedora

Linkwise provides a two-way software modem, station diagnostics, and local
modem simulations. See Station Setup for audio routing and transmit settings.

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

### SELinux

Keep SELinux enforcing. The RPM installs policy for the system service.
After creating or replacing configuration, restore its labels and verify
the running domain:

```sh
sudo restorecon -Rv /etc/linkwise
ls -Z /usr/bin/linkwise-modemd /etc/linkwise/station.json
ps -Z -p "$(systemctl show -p MainPID --value linkwise-modem)"
sudo ausearch -m AVC,USER_AVC -ts recent -i
```

Expect `linkwise_modem_exec_t` on the executable, `linkwise_modem_etc_t` on
configuration, and `linkwise_modem_t` on the system service. The policy
labels the default command and data ports, TCP 8300 and 8301, with
`linkwise_modem_port_t`. An administrator using different ports must
assign that port type and resolve any existing
port-label conflict. The desktop service runs in the caller's user domain.

Fedora enforcement testing and DISA STIG qualification are separate checks.
Qualify the intended STIG-covered OS image, policy variant, and site baseline
before deployment.

## Uninstall

Close running Linkwise commands, then run `sudo dnf remove linkwise-modem`.
Station files saved in your home directory remain available.
