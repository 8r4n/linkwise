# Linkwise-Modemvorschau für Ubuntu

Linkwise bietet ein bidirektionales Softwaremodem, Stationsdiagnosen und lokale Simulationen. Siehe Station Setup für Audioführung und Sendeeinstellungen.

Der Vorschau-Installer enthält `secure-transfer` nicht. Dateiübertragung mit gzip/SHA-256, gemeinsame PKI, geschützter Chat und Mail sowie QSL-Zertifikatkarten erfordern einen Build aus dem Quellcode. Der [PKI-Leitfaden](../../../../../../rust/docs/pki.md) beschreibt Einrichtung und hybriden Chat.

## Installieren oder aktualisieren

Ermitteln Sie mit `dpkg --print-architecture` Ihre Architektur. Führen Sie die Befehle im Downloadverzeichnis aus und setzen Sie den Namen des passenden Pakets ein.

```sh
sudo apt install ./linkwise-modem_0.1.0_arm64.deb
linkwise-modemd --version
linkwise-modemd --simulate
```

APT installiert die benötigten Bibliotheken. Aktualisieren Sie mit demselben Installationsbefehl und der neueren Datei. Die Simulation läuft lokal ohne Funkgerät.

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

## Deinstallieren

Beenden Sie laufende Linkwise-Befehle und führen Sie `sudo apt remove linkwise-modem` aus. Stationsdateien im Benutzerordner bleiben erhalten.
