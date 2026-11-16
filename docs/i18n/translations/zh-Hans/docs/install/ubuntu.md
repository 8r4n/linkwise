# Ubuntu 版 Linkwise 调制解调器预览

Linkwise 提供双向软件调制解调器、台站诊断和本地模拟。音频布线及发射设置请参阅 Station Setup。

预览安装包不含 `secure-transfer`。gzip/SHA-256 文件传输、共享 PKI、受保护的聊天和邮件以及证书 QSL 卡片需要从源代码构建。配置方法和混合聊天的要求见 [PKI 指南](../../../../../../rust/docs/pki.md)。

## 安装或更新

运行 `dpkg --print-architecture` 确认架构。在下载目录执行以下命令，将示例文件名替换为对应软件包的实际文件名。

```sh
sudo apt install ./linkwise-modem_0.1.0_arm64.deb
linkwise-modemd --version
linkwise-modemd --simulate
```

APT 会安装所需系统库。更新时使用同一安装命令，但指定新版文件。模拟在本机运行，无需无线电台。

## 配置台站

使用 `man 8 linkwise-modemd` 查看命令，使用 `man 5 linkwise-station` 查看格式。参阅 `/usr/share/doc/linkwise-modem/Station-setup.md` 选择设备并创建 `station.json`。以桌面用户身份检查音频，以便访问当前音频会话。

## 运行调制解调器服务

桌面台站将配置保存到 `~/.config/linkwise/station.json`，然后运行以下命令。

```sh
systemctl --user enable --now linkwise-modem.service
journalctl --user -u linkwise-modem.service
```

专用台站在 `/etc/linkwise` 下保存由管理员拥有的配置和配置文件。服务必须能够读取文件，并通过 ALSA 直接访问指定的输入和输出设备。每组设备和端口只能由一个服务使用。

```sh
sudo systemctl enable --now linkwise-modem.service
sudo journalctl -u linkwise-modem.service
```

设置 `bandwidth` 后，调制解调器在 IPv4 回环地址打开控制和数据端口。未设置时，控制端口以 JSON 提供接收诊断。使用 `nc 127.0.0.1 8300` 读取。

## 卸载

关闭运行中的 Linkwise 命令，再运行 `sudo apt remove linkwise-modem`。主目录中的台站文件会保留。
