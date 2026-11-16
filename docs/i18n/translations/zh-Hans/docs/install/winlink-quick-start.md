# Linkwise 与 Winlink Express

Windows 向导支持原生音频和无需射频的波形对端。使用原生音频时，选择带宽，并填写 `linkwise-modemd --audio-devices` 显示的准确录音和播放设备名称。在 Express 中使用向导的控制端口和相同带宽。音频布线和电台控制请参阅[台站配置指南](../../../../../../rust/docs/modem-setup.md#run-the-two-way-modem)。

若要进行无需射频的 BW500 测试，请在 Linkwise 源代码仓库运行 `python tools/fetch-modem-profile.py` 生成测量配置文件。

1. 打开 **Configure Linkwise for Winlink**。输入与 Express 中相同的呼号。选择 **Radio-free waveform test** 和 BW500 JSON 配置文件，然后单击 **Apply and start service**。
2. 默认使用控制端口 **8300**。若原版 VARA 已占用该端口，则使用 **18310**。Linkwise 会显示 DATA 端口和波形对端端口，两者是紧随控制端口的两个端口。
3. 将波形对端连接到显示的端口。服务只接受一个本地对端。可选的 `linkwise-modemd --virtual-audio-device` 桥接需要独立的虚拟输入和输出路径。若音频驱动需要已登录会话，请从 Windows 桌面运行该桥接。
4. 在 Express 中打开 **VARA HF P2P**，再打开 **Settings → TNC Setup**。输入 **127.0.0.1** 和向导显示的控制端口。Express 会推导出 DATA 端口。将最大带宽设为 **500**，取消 **Automatically launch TNC when session is opened**。保存后重新打开会话。
5. Express 显示 **Ready** 后，输入隔离对端的呼号和台架试验的中心频率。将带有合成附件的 P2P 消息加入队列。核对接收方附件的 SHA-256，并检查双方会话日志均记录断开连接。

如果 Express 无法连接，请检查地址和控制端口，然后在事件查看器中打开 **Windows Logs → Application**，按 `LinkwiseModem` 筛选。如果已经连接但一直不显示 Ready，请确认波形对端已经连接，双方呼号和带宽一致。
