# Préversion du modem Linkwise pour Fedora

Linkwise fournit un modem logiciel bidirectionnel, des diagnostics de station et des simulations locales. Consultez Station Setup pour le routage audio et les réglages d’émission.

Le paquet de préversion omet `secure-transfer`. Compilez les sources pour utiliser le transfert gzip/SHA-256, la PKI partagée, le chat et le courrier protégés et les cartes QSL de certificats. Le [guide PKI](../../../../../../rust/docs/pki.md) décrit la configuration et le chat hybride.

## Installer ou mettre à jour

Exécutez `uname -m` pour connaître votre architecture. Depuis le répertoire du téléchargement, exécutez les commandes suivantes en remplaçant le nom de fichier par celui du paquet correspondant.

```sh
sudo dnf install ./linkwise-modem-0.1.0-1.aarch64.rpm
linkwise-modemd --version
linkwise-modemd --simulate
```

DNF installe les bibliothèques système nécessaires. Mettez à jour avec `sudo dnf upgrade ./FILE.rpm` et le nouveau fichier. La simulation est locale et sans radio.

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

### SELinux

Gardez SELinux en mode enforcing. Le RPM installe une politique pour le service système. Après création ou remplacement de la configuration, restaurez les étiquettes et vérifiez le domaine actif.

```sh
sudo restorecon -Rv /etc/linkwise
ls -Z /usr/bin/linkwise-modemd /etc/linkwise/station.json
ps -Z -p "$(systemctl show -p MainPID --value linkwise-modem)"
sudo ausearch -m AVC,USER_AVC -ts recent -i
```

Attendez-vous à `linkwise_modem_exec_t` sur l’exécutable, `linkwise_modem_etc_t` sur la configuration et `linkwise_modem_t` sur le service système. La politique attribue `linkwise_modem_port_t` aux ports TCP 8300 et 8301. Pour d’autres ports, attribuez ce type et résolvez les conflits d’étiquettes. Le service de bureau emploie le domaine de son utilisateur.

Les tests SELinux de Fedora et la qualification DISA STIG sont distincts. Qualifiez avant déploiement l’image du système couverte par le STIG, la variante de politique et la configuration de référence du site.

## Désinstaller

Quittez les commandes Linkwise en cours puis exécutez `sudo dnf remove linkwise-modem`. Les fichiers de station dans le dossier personnel restent disponibles.
