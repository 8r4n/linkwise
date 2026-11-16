# Linkwise com Winlink Express

O assistente do Windows aceita áudio nativo e um par de formas de onda sem rádio. Para áudio nativo, selecione a largura de banda e informe os nomes exatos de captura e reprodução mostrados por `linkwise-modemd --audio-devices`. No Express, use a porta de comando e a largura de banda do assistente. Consulte o [guia de configuração da estação](../../../../../../rust/docs/modem-setup.md#run-the-two-way-modem) para roteamento de áudio e controle do rádio.

Para testar BW500 sem rádio, gere o perfil de medição com `python tools/fetch-modem-profile.py` no repositório Linkwise.

1. Abra **Configure Linkwise for Winlink**. Informe o mesmo indicativo usado no Express. Selecione **Radio-free waveform test**, escolha o perfil JSON BW500 e clique em **Apply and start service**.
2. Use a porta de comando **8300**, ou **18310** se o VARA já usar a primeira. O Linkwise mostra as portas DATA e do par de formas de onda. São as duas portas seguintes à de comando.
3. Conecte o par de formas de onda à porta exibida. O serviço aceita um único par local. A ponte opcional `linkwise-modemd --virtual-audio-device` precisa de caminhos virtuais independentes de entrada e saída. Execute-a na área de trabalho do Windows se o driver de áudio exigir uma sessão iniciada.
4. No Express, abra **VARA HF P2P** e depois **Settings → TNC Setup**. Informe **127.0.0.1** e a porta de comando mostrada no assistente. O Express deduz a porta DATA. Selecione **500** como largura de banda máxima e desmarque **Automatically launch TNC when session is opened**. Salve e reabra a sessão.
5. Quando o Express mostrar **Ready**, informe o indicativo do par isolado e uma frequência central para o ensaio. Enfileire uma mensagem P2P com anexo sintético. Compare o SHA-256 do anexo recebido e confirme a desconexão nos dois registros da sessão.

Se o Express não se conectar, confira o endereço e a porta de comando. No Visualizador de Eventos, abra **Windows Logs → Application** e filtre por `LinkwiseModem`. Se conectar sem chegar a Ready, confira se o par de formas de onda está conectado e se os indicativos e a largura de banda coincidem.
