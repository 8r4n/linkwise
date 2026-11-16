# Предварительная версия модема Linkwise для macOS

Linkwise предоставляет двусторонний программный модем, диагностику станции и локальную симуляцию. Маршрутизация звука и настройки передачи описаны в Station Setup.

Установщик предварительной версии не содержит `secure-transfer`. Для передачи файлов с gzip/SHA-256, общей PKI, защищённого чата и почты, а также карточек QSL с сертификатами соберите программу из исходного кода. Настройка и требования гибридного чата описаны в [руководстве по PKI](../../../../../../rust/docs/pki.md).

## Установка или обновление

1. В меню Apple → Об этом Mac проверьте чип или процессор. Для Apple silicon выберите `macos-arm64.pkg`, для Intel — `macos-x86_64.pkg`.
2. Откройте пакет в программе установки и выполните её указания. Требуется учётная запись администратора. Обновите программу таким же образом, открыв новый пакет. Храните конфигурацию станции в домашнем каталоге.
3. Откройте Терминал и выполните следующие команды.

   ```sh
   /usr/local/bin/linkwise-modemd --version
   /usr/local/bin/linkwise-modemd --simulate
   ```

Симуляция выполняется локально без радио. Пакеты для разработки могут не иметь подписи. Если macOS отклоняет пакет, запросите у поставщика подписанную и нотариально заверенную сборку.

## Настройка станции

Выполните `man 8 linkwise-modemd` для справки о командах и `man 5 linkwise-station` для справки о конфигурации. Откройте `/usr/local/share/doc/linkwise-modem/Station-setup.md`, чтобы выбрать устройства и создать `station.json`. Если запись запрещена, проверьте разрешение Терминала в «Системные настройки → Конфиденциальность и безопасность → Микрофон».

## Запуск агента входа для модема

В macOS 13 и новее сохраните конфигурацию в `~/Library/Application Support/Linkwise/station.json` и выполните следующую команду.

```sh
"/Applications/Linkwise Station.app/Contents/MacOS/LinkwiseStation" enable
```

По запросу разрешите доступ к микрофону и одобрите фоновый элемент в системных настройках. Та же команда с `status` показывает состояние регистрации. Перед удалением отмените регистрацию агента с помощью `disable`.

## Удаление

Завершите работающие команды Linkwise, затем удалите установленные файлы и квитанцию пакета.

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

Файлы станции в домашнем каталоге сохранятся.
