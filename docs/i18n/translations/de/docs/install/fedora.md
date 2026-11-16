# Linkwise-Modemvorschau für Fedora

Linkwise bietet ein bidirektionales Softwaremodem, Stationsdiagnosen und lokale Simulationen. Siehe Station Setup für Audioführung und Sendeeinstellungen.

Der Vorschau-Installer enthält `secure-transfer` nicht. Dateiübertragung mit gzip/SHA-256, gemeinsame PKI, geschützter Chat und Mail sowie QSL-Zertifikatkarten erfordern einen Build aus dem Quellcode. Der [PKI-Leitfaden](../../../../../../rust/docs/pki.md) beschreibt Einrichtung und hybriden Chat.

## Installieren oder aktualisieren

Ermitteln Sie mit `uname -m` Ihre Architektur. Führen Sie die Befehle im Downloadverzeichnis aus und setzen Sie den Namen des passenden Pakets ein.

```sh
sudo dnf install ./linkwise-modem-0.1.0-1.aarch64.rpm
linkwise-modemd --version
linkwise-modemd --simulate
```

DNF installiert die benötigten Bibliotheken. Verwenden Sie zur Aktualisierung `sudo dnf upgrade ./FILE.rpm` mit der neueren Datei. Die Simulation läuft lokal ohne Funkgerät.

## Station einrichten

Verwenden Sie `man 8 linkwise-modemd` für Befehle und `man 5 linkwise-station` für das Format. `/usr/share/doc/linkwise-modem/Station-setup.md` erklärt Geräteauswahl und `station.json`. Führen Sie Audiotests als Desktopbenutzer aus, damit die Audiositzung erreichbar ist.

## Modemdienst ausführen

Speichern Sie für eine Desktopstation die Konfiguration in `~/.config/linkwise/station.json` und führen Sie die folgenden Befehle aus.

```sh
systemctl --user enable --now linkwise-modem.service
journalctl --user -u linkwise-modem.service
```

Bei einer eigenen Station legen Sie Konfiguration und Profile im Besitz des Administrators unter `/etc/linkwise` ab. Der Dienst braucht Lesezugriff und direkten ALSA-Zugriff auf die konfigurierten Geräte. Verwenden Sie nur einen Dienst pro Geräte- und Portkombination.

```sh
sudo systemctl enable --now linkwise-modem.service
sudo journalctl -u linkwise-modem.service
```

Mit `bandwidth` öffnet das Modem beide Ports auf der IPv4-Loopback-Adresse. Ohne das Feld liefert der Befehlsport Empfangsdiagnosen als JSON. Rufen Sie sie mit `nc 127.0.0.1 8300` ab.

### SELinux

Belassen Sie SELinux im Modus enforcing. Das RPM installiert die Richtlinie des Systemdienstes. Stellen Sie nach Erstellen oder Ersetzen der Konfiguration die Labels wieder her und prüfen Sie die aktive Domäne.

```sh
sudo restorecon -Rv /etc/linkwise
ls -Z /usr/bin/linkwise-modemd /etc/linkwise/station.json
ps -Z -p "$(systemctl show -p MainPID --value linkwise-modem)"
sudo ausearch -m AVC,USER_AVC -ts recent -i
```

Erwartet werden `linkwise_modem_exec_t` für das Programm, `linkwise_modem_etc_t` für die Konfiguration und `linkwise_modem_t` für den Systemdienst. TCP 8300 und 8301 erhalten `linkwise_modem_port_t`. Weisen Sie bei anderen Ports diesen Typ zu und lösen Sie Labelkonflikte. Der Desktopdienst läuft in der Domäne des aufrufenden Benutzers.

Fedora-SELinux-Tests und DISA-STIG-Qualifikation sind getrennte Prüfungen. Qualifizieren Sie vor dem Einsatz das abgedeckte Betriebssystemabbild, die Richtlinienvariante und die Standort-Basiskonfiguration.

## Deinstallieren

Beenden Sie laufende Linkwise-Befehle und führen Sie `sudo dnf remove linkwise-modem` aus. Stationsdateien im Benutzerordner bleiben erhalten.
