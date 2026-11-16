# Préversion du modem Linkwise pour macOS

Linkwise fournit un modem logiciel bidirectionnel, des diagnostics de station et des simulations locales. Consultez Station Setup pour le routage audio et les réglages d’émission.

Le paquet de préversion omet `secure-transfer`. Compilez les sources pour utiliser le transfert gzip/SHA-256, la PKI partagée, le chat et le courrier protégés et les cartes QSL de certificats. Le [guide PKI](../../../../../../rust/docs/pki.md) décrit la configuration et le chat hybride.

## Installer ou mettre à jour

1. Dans Menu Apple → À propos de ce Mac, vérifiez la puce ou le processeur. Choisissez `macos-arm64.pkg` pour Apple silicon ou `macos-x86_64.pkg` pour Intel.
2. Ouvrez le paquet dans Programme d’installation et suivez les instructions. Un compte administrateur est requis. Pour mettre à jour, ouvrez le nouveau paquet de la même manière. Conservez la configuration de station dans votre dossier personnel.
3. Ouvrez Terminal et exécutez les commandes suivantes.

   ```sh
   /usr/local/bin/linkwise-modemd --version
   /usr/local/bin/linkwise-modemd --simulate
   ```

La simulation s’exécute localement sans radio. Les paquets de développement peuvent être non signés. Si macOS refuse le paquet, demandez au distributeur une version signée et notarisée.

## Configurer la station

Exécutez `man 8 linkwise-modemd` pour les commandes et `man 5 linkwise-station` pour le format de configuration. Ouvrez `/usr/local/share/doc/linkwise-modem/Station-setup.md` pour choisir les appareils et créer `station.json`. Si la capture est refusée, vérifiez Réglages Système → Confidentialité et sécurité → Microphone pour Terminal.

## Exécuter l’agent de connexion du modem

Sous macOS 13 ou ultérieur, enregistrez la configuration dans `~/Library/Application Support/Linkwise/station.json` et exécutez la commande suivante.

```sh
"/Applications/Linkwise Station.app/Contents/MacOS/LinkwiseStation" enable
```

Autorisez le microphone et l’élément en arrière-plan dans Réglages Système si nécessaire. Utilisez la même commande avec `status` pour connaître l’état, ou `disable` pour désinscrire l’agent avant la désinstallation.

## Désinstaller

Quittez toutes les commandes Linkwise, puis supprimez les fichiers installés et le reçu du paquet.

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

Les fichiers de station de votre dossier personnel restent disponibles.
