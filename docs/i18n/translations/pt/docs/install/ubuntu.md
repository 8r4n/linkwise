# Prévia do modem Linkwise para Ubuntu

O Linkwise fornece um modem de software bidirecional, diagnóstico de estação e simulações locais. Consulte Station Setup para roteamento de áudio e ajustes de transmissão.

O instalador de prévia omite `secure-transfer`. Compile o código-fonte para transferir arquivos com gzip/SHA-256, compartilhar PKI, proteger conversa e correio e gerar cartões QSL de certificados. O [guia PKI](../../../../../../rust/docs/pki.md) descreve a configuração e a conversa híbrida.

## Instalar ou atualizar

Execute `dpkg --print-architecture` para identificar sua arquitetura. No diretório do download, execute os comandos substituindo o nome do arquivo pelo do pacote correspondente.

```sh
sudo apt install ./linkwise-modem_0.1.0_arm64.deb
linkwise-modemd --version
linkwise-modemd --simulate
```

APT instala as bibliotecas necessárias. Para atualizar, use o mesmo comando com o arquivo novo. A simulação é local e sem rádio.

## Configurar a estação

Use `man 8 linkwise-modemd` para os comandos e `man 5 linkwise-station` para o formato. Consulte `/usr/share/doc/linkwise-modem/Station-setup.md` para selecionar dispositivos e criar `station.json`. Faça os testes como usuário da sessão de desktop para acessar o áudio.

## Executar o serviço do modem

Em uma estação de desktop, salve a configuração em `~/.config/linkwise/station.json` e execute os comandos abaixo.

```sh
systemctl --user enable --now linkwise-modem.service
journalctl --user -u linkwise-modem.service
```

Para uma estação dedicada, coloque a configuração e os perfis pertencentes ao administrador em `/etc/linkwise`. O serviço precisa ler os arquivos e acessar diretamente as entradas e saídas ALSA configuradas. Use apenas um serviço para os mesmos dispositivos e portas.

```sh
sudo systemctl enable --now linkwise-modem.service
sudo journalctl -u linkwise-modem.service
```

Com `bandwidth` configurado, o modem abre as duas portas no loopback IPv4. Sem o campo, a porta de comando fornece diagnóstico de recepção em JSON. Consulte com `nc 127.0.0.1 8300`.

## Desinstalar

Feche os comandos Linkwise em execução e rode `sudo apt remove linkwise-modem`. Os arquivos da estação na pasta pessoal permanecem disponíveis.
