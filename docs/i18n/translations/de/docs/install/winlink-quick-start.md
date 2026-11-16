# Linkwise mit Winlink Express

Der Windows-Assistent unterstützt natives Audio und eine funkfreie Wellenform-Gegenstelle. Wählen Sie für natives Audio eine Bandbreite und geben Sie die exakten Aufnahme- und Wiedergabegeräte aus `linkwise-modemd --audio-devices` ein. Übernehmen Sie Befehlsport und Bandbreite aus dem Assistenten in Express. Hinweise zur Audioführung und Funksteuerung finden Sie in der [Stationsanleitung](../../../../../../rust/docs/modem-setup.md#run-the-two-way-modem).

Erzeugen Sie für den funkfreien BW500-Test im Linkwise-Repository mit `python tools/fetch-modem-profile.py` ein Messprofil.

1. Öffnen Sie **Configure Linkwise for Winlink**. Geben Sie das Rufzeichen aus Express ein. Wählen Sie **Radio-free waveform test** und das BW500-JSON-Profil. Klicken Sie auf **Apply and start service**.
2. Verwenden Sie den Befehlsport **8300**, oder **18310**, falls VARA bereits 8300 belegt. Linkwise zeigt den DATA-Port und den Port zur Wellenform-Gegenstelle an. Es handelt sich um die beiden auf den Befehlsport folgenden Ports.
3. Verbinden Sie die Wellenform-Gegenstelle mit dem angezeigten Port. Der Dienst akzeptiert eine lokale Gegenstelle. Die optionale Brücke `linkwise-modemd --virtual-audio-device` benötigt getrennte virtuelle Ein- und Ausgabepfade. Starten Sie sie vom Windows-Desktop, wenn der Audiotreiber eine angemeldete Sitzung verlangt.
4. Öffnen Sie in Express **VARA HF P2P** und dann **Settings → TNC Setup**. Geben Sie **127.0.0.1** und den Befehlsport des Assistenten ein. Express leitet den DATA-Port daraus ab. Wählen Sie **500** als maximale Bandbreite und deaktivieren Sie **Automatically launch TNC when session is opened**. Speichern Sie und öffnen Sie die Sitzung erneut.
5. Sobald Express **Ready** anzeigt, geben Sie das Rufzeichen der isolierten Gegenstelle und eine Mittenfrequenz für den Test ein. Stellen Sie eine P2P-Nachricht mit synthetischem Anhang in die Warteschlange. Prüfen Sie den SHA-256 des empfangenen Anhangs und das Verbindungsende in beiden Sitzungsprotokollen.

Wenn Express keine Verbindung herstellt, prüfen Sie Adresse und Befehlsport. Öffnen Sie in der Ereignisanzeige **Windows Logs → Application** und filtern Sie nach `LinkwiseModem`. Wenn die Verbindung hergestellt wird, aber Ready ausbleibt, prüfen Sie die Wellenform-Gegenstelle sowie die Rufzeichen und die Bandbreite.
