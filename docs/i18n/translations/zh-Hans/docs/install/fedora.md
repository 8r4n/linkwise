# Fedora 版 Linkwise 调制解调器预览

Linkwise 提供双向软件调制解调器、台站诊断和本地模拟。音频布线及发射设置请参阅 Station Setup。

预览安装包不含 `secure-transfer`。gzip/SHA-256 文件传输、共享 PKI、受保护的聊天和邮件以及证书 QSL 卡片需要从源代码构建。配置方法和混合聊天的要求见 [PKI 指南](../../../../../../rust/docs/pki.md)。

## 安装或更新

运行 `uname -m` 确认架构。在下载目录执行以下命令，将示例文件名替换为对应软件包的实际文件名。

```sh
sudo dnf install ./linkwise-modem-0.1.0-1.aarch64.rpm
linkwise-modemd --version
linkwise-modemd --simulate
```

DNF 会安装所需系统库。使用 `sudo dnf upgrade ./FILE.rpm` 指定新版文件进行更新。模拟在本机运行，无需无线电台。

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

### SELinux

请保持 SELinux 强制模式。RPM 会安装系统服务策略。创建或更换配置后，恢复标签，并核对正在运行的域。

```sh
sudo restorecon -Rv /etc/linkwise
ls -Z /usr/bin/linkwise-modemd /etc/linkwise/station.json
ps -Z -p "$(systemctl show -p MainPID --value linkwise-modem)"
sudo ausearch -m AVC,USER_AVC -ts recent -i
```

可执行文件应为 `linkwise_modem_exec_t`，配置为 `linkwise_modem_etc_t`，系统服务为 `linkwise_modem_t`。默认 TCP 8300 和 8301 由策略标为 `linkwise_modem_port_t`。若使用其他端口，请分配这一类型并解决标签冲突。桌面服务使用启动用户的域。

Fedora 强制策略测试与 DISA STIG 资格验证是独立工作。部署前请验证受 STIG 覆盖的系统镜像、策略变体和现场基线。

## 卸载

关闭运行中的 Linkwise 命令，再运行 `sudo dnf remove linkwise-modem`。主目录中的台站文件会保留。
