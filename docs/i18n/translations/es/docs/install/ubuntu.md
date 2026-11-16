# Versión preliminar del módem Linkwise para Ubuntu

Linkwise ofrece un módem bidireccional, diagnósticos de estación y simulaciones locales. Consulta Station Setup para el audio y los ajustes de transmisión.

Este instalador preliminar omite `secure-transfer`. Compila desde el código fuente para transferencias gzip/SHA-256, PKI compartida, chat y correo protegidos y tarjetas QSL con certificados. La [guía PKI](../../../../../../rust/docs/pki.md) explica la configuración y el chat híbrido.

## Instalar o actualizar

Ejecuta `dpkg --print-architecture` para conocer tu arquitectura. En la carpeta de descarga, ejecuta los comandos y usa el nombre del paquete correspondiente.

```sh
sudo apt install ./linkwise-modem_0.1.0_arm64.deb
linkwise-modemd --version
linkwise-modemd --simulate
```

APT instala las bibliotecas necesarias. Para actualizar, ejecuta el mismo comando con el archivo nuevo. La simulación es local y no requiere radio.

## Configurar la estación

Usa `man 8 linkwise-modemd` para los comandos y `man 5 linkwise-station` para el formato. Abre `/usr/share/doc/linkwise-modem/Station-setup.md` para elegir dispositivos y crear `station.json`. Ejecuta las pruebas como usuario de escritorio para acceder a su sesión de audio.

## Ejecutar el servicio del módem

Para una estación de escritorio, guarda la configuración en `~/.config/linkwise/station.json` y ejecuta los comandos siguientes.

```sh
systemctl --user enable --now linkwise-modem.service
journalctl --user -u linkwise-modem.service
```

Para una estación dedicada, instala los archivos de configuración y perfil bajo `/etc/linkwise` con el administrador como propietario. El servicio necesita leerlos y acceder directamente a las entradas y salidas ALSA configuradas. Usa un solo servicio para cada conjunto de dispositivos y puertos.

```sh
sudo systemctl enable --now linkwise-modem.service
sudo journalctl -u linkwise-modem.service
```

Con `bandwidth`, el módem abre ambos puertos en el bucle local IPv4. Si omites el campo, el puerto de comandos ofrece el diagnóstico de recepción en JSON. Léelo con `nc 127.0.0.1 8300`.

## Desinstalar

Cierra los comandos Linkwise activos y ejecuta `sudo apt remove linkwise-modem`. Los archivos de estación en la carpeta personal se conservan.
