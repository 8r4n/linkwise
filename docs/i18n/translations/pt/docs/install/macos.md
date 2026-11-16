# Prévia do modem Linkwise para macOS

O Linkwise fornece um modem de software bidirecional, diagnóstico de estação e simulações locais. Consulte Station Setup para roteamento de áudio e ajustes de transmissão.

O instalador de prévia omite `secure-transfer`. Compile o código-fonte para transferir arquivos com gzip/SHA-256, compartilhar PKI, proteger conversa e correio e gerar cartões QSL de certificados. O [guia PKI](../../../../../../rust/docs/pki.md) descreve a configuração e a conversa híbrida.

## Instalar ou atualizar

1. Em Menu Apple → Sobre Este Mac, confira o chip ou processador. Escolha `macos-arm64.pkg` para Apple silicon ou `macos-x86_64.pkg` para Intel.
2. Abra o pacote no Instalador e siga as instruções usando uma conta de administrador. Para atualizar, execute o novo pacote da mesma maneira. Guarde a configuração da estação na pasta pessoal.
3. Abra o Terminal e execute os comandos abaixo.

   ```sh
   /usr/local/bin/linkwise-modemd --version
   /usr/local/bin/linkwise-modemd --simulate
   ```

A simulação é local e funciona sem rádio. Pacotes de desenvolvimento podem ser distribuídos sem assinatura. Se o macOS recusar o pacote, peça uma compilação assinada e autenticada ao distribuidor.

## Configurar a estação

Execute `man 8 linkwise-modemd` para os comandos e `man 5 linkwise-station` para o formato. Abra `/usr/local/share/doc/linkwise-modem/Station-setup.md` para selecionar os dispositivos e criar `station.json`. Se a captura for negada, verifique Ajustes do Sistema → Privacidade e Segurança → Microfone para o Terminal.

## Executar o agente de início de sessão do modem

No macOS 13 ou posterior, guarde a configuração em `~/Library/Application Support/Linkwise/station.json` e execute o comando abaixo.

```sh
"/Applications/Linkwise Station.app/Contents/MacOS/LinkwiseStation" enable
```

Se solicitado, permita o microfone e aprove o item em segundo plano nos Ajustes do Sistema. Use o mesmo comando com `status` para consultar o registro ou `disable` para cancelar o registro antes de desinstalar.

## Desinstalar

Feche os comandos Linkwise em execução e remova os arquivos instalados e o recibo do pacote.

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

Os arquivos da estação na sua pasta pessoal permanecem disponíveis.
