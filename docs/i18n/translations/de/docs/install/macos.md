# Linkwise-Modemvorschau für macOS

Linkwise bietet ein bidirektionales Softwaremodem, Stationsdiagnosen und lokale Simulationen. Siehe Station Setup für Audioführung und Sendeeinstellungen.

Der Vorschau-Installer enthält `secure-transfer` nicht. Dateiübertragung mit gzip/SHA-256, gemeinsame PKI, geschützter Chat und Mail sowie QSL-Zertifikatkarten erfordern einen Build aus dem Quellcode. Der [PKI-Leitfaden](../../../../../../rust/docs/pki.md) beschreibt Einrichtung und hybriden Chat.

## Installieren oder aktualisieren

1. Prüfen Sie in Apple-Menü → Über diesen Mac den Chip oder Prozessor. Wählen Sie `macos-arm64.pkg` für Apple-Chips oder `macos-x86_64.pkg` für Intel.
2. Öffnen Sie das Paket im Installationsprogramm. Folgen Sie den Anweisungen mit einem Administratorkonto. Aktualisieren Sie auf dieselbe Weise mit dem neueren Paket. Bewahren Sie die Stationskonfiguration im Benutzerordner auf.
3. Öffnen Sie Terminal und führen Sie die folgenden Befehle aus.

   ```sh
   /usr/local/bin/linkwise-modemd --version
   /usr/local/bin/linkwise-modemd --simulate
   ```

Die Simulation läuft lokal ohne Funkgerät. Entwicklungspakete können unsigniert sein. Wenn macOS das Paket ablehnt, fragen Sie den Anbieter nach einem signierten und notarisierten Build.

## Station einrichten

Öffnen Sie die Befehlsreferenz mit `man 8 linkwise-modemd` und das Konfigurationsformat mit `man 5 linkwise-station`. Die Datei `/usr/local/share/doc/linkwise-modem/Station-setup.md` erklärt die Geräteauswahl und `station.json`. Bei verweigerter Aufnahme prüfen Sie Systemeinstellungen → Datenschutz & Sicherheit → Mikrofon für Terminal.

## Modem-Anmeldeagent ausführen

Unter macOS 13 oder neuer speichern Sie die Konfiguration in `~/Library/Application Support/Linkwise/station.json`. Führen Sie anschließend den folgenden Befehl aus.

```sh
"/Applications/Linkwise Station.app/Contents/MacOS/LinkwiseStation" enable
```

Erlauben Sie bei Bedarf den Mikrofonzugriff und das Hintergrundobjekt in den Systemeinstellungen. Verwenden Sie denselben Befehl mit `status` für den Registrierungsstatus oder mit `disable`, um die Registrierung vor der Deinstallation aufzuheben.

## Deinstallieren

Beenden Sie laufende Linkwise-Befehle. Entfernen Sie die installierten Dateien und den Paketbeleg.

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

Stationsdateien im Benutzerordner bleiben erhalten.
