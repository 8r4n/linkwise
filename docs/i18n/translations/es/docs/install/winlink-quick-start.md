# Linkwise con Winlink Express

El asistente de Windows admite audio nativo y un par de formas de onda sin radio. Para audio nativo, elige un ancho de banda e introduce los nombres exactos de los dispositivos de captura y reproducción que muestra `linkwise-modemd --audio-devices`. Usa en Express el puerto de comandos y el ancho de banda indicados por el asistente. Consulta la [guía de configuración de la estación](../../../../../../rust/docs/modem-setup.md#run-the-two-way-modem) para el audio y el control de la radio.

Para la prueba BW500 sin radio, genera un perfil de medición desde el repositorio de Linkwise con `python tools/fetch-modem-profile.py`.

1. Abre **Configure Linkwise for Winlink**. Introduce el mismo indicativo que usas en Express. Selecciona **Radio-free waveform test**, elige el perfil JSON de BW500 y pulsa **Apply and start service**.
2. Usa el puerto de comandos **8300**, o **18310** si VARA ya ocupa el primero. Linkwise muestra el puerto DATA y el del par de formas de onda. Son los dos puertos siguientes al de comandos.
3. Conecta el par de formas de onda al puerto que se muestra. El servicio acepta un par local. El puente opcional `linkwise-modemd --virtual-audio-device` necesita rutas virtuales independientes de entrada y salida. Si el controlador de audio requiere una sesión iniciada, ejecútalo desde el escritorio de Windows.
4. En Express, abre **VARA HF P2P** y luego **Settings → TNC Setup**. Introduce **127.0.0.1** y el puerto de comandos del asistente. Express deduce el puerto DATA. Elige **500** como ancho de banda máximo y desactiva **Automatically launch TNC when session is opened**. Guarda y vuelve a abrir la sesión.
5. Cuando Express muestre **Ready**, introduce el indicativo del par aislado y una frecuencia central para la prueba. Pon en cola un mensaje P2P con un adjunto sintético. Comprueba el SHA-256 del adjunto recibido y que ambos registros indiquen la desconexión.

Si Express no se conecta, comprueba la dirección y el puerto de comandos. En el Visor de eventos, abre **Windows Logs → Application** y filtra por `LinkwiseModem`. Si se conecta pero no llega a Ready, comprueba que el par de formas de onda esté conectado y que coincidan los indicativos y el ancho de banda.
