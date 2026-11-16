# Prévia do modem Linkwise para Fedora

O Linkwise fornece um modem de software bidirecional, diagnóstico de estação e simulações locais. Consulte Station Setup para roteamento de áudio e ajustes de transmissão.

O instalador de prévia omite `secure-transfer`. Compile o código-fonte para transferir arquivos com gzip/SHA-256, compartilhar PKI, proteger conversa e correio e gerar cartões QSL de certificados. O [guia PKI](../../../../../../rust/docs/pki.md) descreve a configuração e a conversa híbrida.

## Instalar ou atualizar

Execute `uname -m` para identificar sua arquitetura. No diretório do download, execute os comandos substituindo o nome do arquivo pelo do pacote correspondente.

```sh
sudo dnf install ./linkwise-modem-0.1.0-1.aarch64.rpm
linkwise-modemd --version
linkwise-modemd --simulate
```

DNF instala as bibliotecas necessárias. Para atualizar, use `sudo dnf upgrade ./FILE.rpm` com o arquivo novo. A simulação é local e sem rádio.

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

### SELinux

Mantenha SELinux em enforcing. O RPM instala uma política para o serviço de sistema. Depois de criar ou substituir a configuração, restaure os rótulos e confira o domínio em execução.

```sh
sudo restorecon -Rv /etc/linkwise
ls -Z /usr/bin/linkwise-modemd /etc/linkwise/station.json
ps -Z -p "$(systemctl show -p MainPID --value linkwise-modem)"
sudo ausearch -m AVC,USER_AVC -ts recent -i
```

Espere `linkwise_modem_exec_t` no executável, `linkwise_modem_etc_t` na configuração e `linkwise_modem_t` no serviço de sistema. A política aplica `linkwise_modem_port_t` às portas TCP 8300 e 8301. Em outras portas, atribua esse tipo e resolva conflitos de rótulos. O serviço de desktop roda no domínio do usuário.

O teste de SELinux no Fedora e a qualificação DISA STIG são verificações separadas. Qualifique antes da implantação a imagem coberta pelo STIG, a variante da política e a configuração de referência do local.

## Desinstalar

Feche os comandos Linkwise em execução e rode `sudo dnf remove linkwise-modem`. Os arquivos da estação na pasta pessoal permanecem disponíveis.
