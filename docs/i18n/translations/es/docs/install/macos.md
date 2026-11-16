# Versión preliminar del módem Linkwise para macOS

Linkwise ofrece un módem bidireccional, diagnósticos de estación y simulaciones locales. Consulta Station Setup para el audio y los ajustes de transmisión.

Este instalador preliminar omite `secure-transfer`. Compila desde el código fuente para transferencias gzip/SHA-256, PKI compartida, chat y correo protegidos y tarjetas QSL con certificados. La [guía PKI](../../../../../../rust/docs/pki.md) explica la configuración y el chat híbrido.

## Instalar o actualizar

1. En Menú Apple → Acerca de este Mac, comprueba el chip o procesador. Elige `macos-arm64.pkg` para Apple silicon o `macos-x86_64.pkg` para Intel.
2. Abre el paquete en el Instalador y sigue los pasos. Necesitas una cuenta de administrador. Instala paquetes posteriores del mismo modo. Guarda la configuración de estación en tu carpeta personal.
3. Abre Terminal y ejecuta los comandos siguientes.

   ```sh
   /usr/local/bin/linkwise-modemd --version
   /usr/local/bin/linkwise-modemd --simulate
   ```

La simulación se ejecuta localmente y sin radio. Los paquetes de desarrollo pueden carecer de firma. Si macOS lo rechaza, pide al distribuidor una compilación firmada y notarizada.

## Configurar la estación

Ejecuta `man 8 linkwise-modemd` para los comandos y `man 5 linkwise-station` para el formato. Abre `/usr/local/share/doc/linkwise-modem/Station-setup.md` para elegir dispositivos y crear `station.json`. Si se deniega la captura, revisa Ajustes del Sistema → Privacidad y seguridad → Micrófono para Terminal.

## Ejecutar el agente de inicio de sesión del módem

En macOS 13 o posterior, guarda la configuración en `~/Library/Application Support/Linkwise/station.json` y ejecuta este comando.

```sh
"/Applications/Linkwise Station.app/Contents/MacOS/LinkwiseStation" enable
```

Si se solicita, da acceso al micrófono y permite el elemento en segundo plano en Ajustes del Sistema. Usa el mismo comando con `status` para consultar el registro o con `disable` para quitarlo antes de desinstalar.

## Desinstalar

Cierra los comandos de Linkwise activos y retira los archivos instalados y el recibo del paquete.

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

Los archivos de estación guardados en tu carpeta personal se conservan.
