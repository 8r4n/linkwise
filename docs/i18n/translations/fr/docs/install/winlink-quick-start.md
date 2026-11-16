# Linkwise avec Winlink Express

L’assistant Windows prend en charge l’audio natif et les essais avec un pair de formes d’onde sans radio. En mode natif, choisissez une largeur de bande et saisissez les noms exacts des périphériques de capture et de lecture indiqués par `linkwise-modemd --audio-devices`. Dans Express, utilisez le port de commande et la largeur de bande de l’assistant. Consultez le [guide de configuration de la station](../../../../../../rust/docs/modem-setup.md#run-the-two-way-modem) pour le routage audio et le contrôle de la radio.

Pour tester BW500 sans radio, générez le profil de mesure depuis le dépôt Linkwise avec `python tools/fetch-modem-profile.py`.

1. Ouvrez **Configure Linkwise for Winlink**. Saisissez le même indicatif que dans Express. Sélectionnez **Radio-free waveform test**, choisissez le profil JSON BW500 et cliquez sur **Apply and start service**.
2. Utilisez le port de commande **8300**, ou **18310** si VARA l’utilise déjà. Linkwise affiche le port DATA et le port du pair de formes d’onde. Ce sont les deux ports qui suivent le port de commande.
3. Connectez le pair de formes d’onde au port indiqué. Le service accepte un seul pair local. La passerelle facultative `linkwise-modemd --virtual-audio-device` exige des chemins virtuels d’entrée et de sortie distincts. Si le pilote audio exige une session ouverte, lancez-la depuis le bureau Windows.
4. Dans Express, ouvrez **VARA HF P2P**, puis **Settings → TNC Setup**. Saisissez **127.0.0.1** et le port de commande affiché par l’assistant. Express déduit le port DATA. Choisissez **500** comme largeur de bande maximale et décochez **Automatically launch TNC when session is opened**. Enregistrez puis rouvrez la session.
5. Lorsque Express affiche **Ready**, indiquez l’indicatif du pair isolé et la fréquence centrale de l’essai. Mettez en file d’attente un message P2P avec une pièce jointe synthétique. Vérifiez son SHA-256 chez le destinataire et confirmez la déconnexion dans les deux journaux de session.

Si Express ne se connecte pas, vérifiez l’adresse et le port de commande. Dans l’Observateur d’événements, ouvrez **Windows Logs → Application** et filtrez sur `LinkwiseModem`. Si Express se connecte sans atteindre Ready, vérifiez que le pair de formes d’onde est relié et que les indicatifs et la largeur de bande concordent.
