# Préversion du modem Linkwise pour Ubuntu

Linkwise fournit un modem logiciel bidirectionnel, des diagnostics de station et des simulations locales. Consultez Station Setup pour le routage audio et les réglages d’émission.

Le paquet de préversion omet `secure-transfer`. Compilez les sources pour utiliser le transfert gzip/SHA-256, la PKI partagée, le chat et le courrier protégés et les cartes QSL de certificats. Le [guide PKI](../../../../../../rust/docs/pki.md) décrit la configuration et le chat hybride.

## Installer ou mettre à jour

Exécutez `dpkg --print-architecture` pour connaître votre architecture. Depuis le répertoire du téléchargement, exécutez les commandes suivantes en remplaçant le nom de fichier par celui du paquet correspondant.

```sh
sudo apt install ./linkwise-modem_0.1.0_arm64.deb
linkwise-modemd --version
linkwise-modemd --simulate
```

APT installe les bibliothèques système nécessaires. Pour mettre à jour, relancez la même commande avec le fichier récent. La simulation est locale et sans radio.

## Configurer la station

Ouvrez `man 8 linkwise-modemd` pour les commandes et `man 5 linkwise-station` pour le format. Consultez `/usr/share/doc/linkwise-modem/Station-setup.md` pour choisir les appareils et créer `station.json`. Vérifiez l’audio en tant qu’utilisateur de la session graphique afin d’accéder à son service audio.

## Exécuter le service modem

Pour une station de bureau, enregistrez la configuration dans `~/.config/linkwise/station.json` puis exécutez les commandes suivantes.

```sh
systemctl --user enable --now linkwise-modem.service
journalctl --user -u linkwise-modem.service
```

Pour une station dédiée, placez sous `/etc/linkwise` les fichiers de configuration et de profil appartenant à l’administrateur. Le service doit pouvoir les lire et accéder directement aux entrées et sorties ALSA configurées. N’utilisez qu’un service pour les mêmes appareils et la même paire de ports.

```sh
sudo systemctl enable --now linkwise-modem.service
sudo journalctl -u linkwise-modem.service
```

Avec `bandwidth`, le modem ouvre les deux ports sur le bouclage IPv4. Sans ce champ, le port de commande renvoie le diagnostic de réception en JSON. Lisez-le avec `nc 127.0.0.1 8300`.

## Désinstaller

Quittez les commandes Linkwise en cours puis exécutez `sudo apt remove linkwise-modem`. Les fichiers de station dans le dossier personnel restent disponibles.
