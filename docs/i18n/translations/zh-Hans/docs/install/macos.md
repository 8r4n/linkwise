# macOS 版 Linkwise 调制解调器预览

Linkwise 提供双向软件调制解调器、台站诊断和本地模拟。音频布线及发射设置请参阅 Station Setup。

预览安装包不含 `secure-transfer`。gzip/SHA-256 文件传输、共享 PKI、受保护的聊天和邮件以及证书 QSL 卡片需要从源代码构建。配置方法和混合聊天的要求见 [PKI 指南](../../../../../../rust/docs/pki.md)。

## 安装或更新

1. 在 Apple 菜单 → 关于本机中确认芯片或处理器。Apple 芯片使用 `macos-arm64.pkg`，Intel 使用 `macos-x86_64.pkg`。
2. 通过“安装器”打开软件包并依提示操作，需要管理员账户。更新时以同样方式运行新版软件包。将台站配置保存在个人主目录。
3. 打开“终端”并运行以下命令。

   ```sh
   /usr/local/bin/linkwise-modemd --version
   /usr/local/bin/linkwise-modemd --simulate
   ```

模拟在本机运行，无需无线电台。开发软件包可能未签名。如果 macOS 拒绝该软件包，请向分发方索要已签名且通过公证的版本。

## 配置台站

使用 `man 8 linkwise-modemd` 查看命令参考，使用 `man 5 linkwise-station` 查看台站配置格式。打开 `/usr/local/share/doc/linkwise-modem/Station-setup.md`，按说明选择设备并创建 `station.json`。若录音被拒绝，请检查“系统设置 → 隐私与安全性 → 麦克风”中“终端”的权限。

## 运行调制解调器登录代理

在 macOS 13 或更高版本中，将台站配置保存到 `~/Library/Application Support/Linkwise/station.json`，然后运行以下命令。

```sh
"/Applications/Linkwise Station.app/Contents/MacOS/LinkwiseStation" enable
```

如果系统提示，请授予麦克风权限并在“系统设置”中批准后台项目。将同一命令的参数改为 `status` 可查看注册状态，卸载前可使用 `disable` 取消注册。

## 卸载

退出所有正在运行的 Linkwise 命令，然后删除已安装文件和软件包收据。

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

个人主目录中的台站文件会保留。
