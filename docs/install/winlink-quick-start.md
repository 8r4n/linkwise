# Linkwise with Winlink Express

The Windows wizard supports native audio and a radio-free waveform peer.
For native audio, select a bandwidth and enter the exact capture and playback
device names from `linkwise-modemd --audio-devices`. Use the wizard's command
port and matching bandwidth in Express. See the
[station setup guide](../../rust/docs/modem-setup.md#run-the-two-way-modem)
for audio routing and radio control.

For the radio-free BW500 test, generate a measurement profile with
`python tools/fetch-modem-profile.py` from the Linkwise repository.

1. Open **Configure Linkwise for Winlink**. Enter the same callsign you use in
   Express. Select **Radio-free waveform test**, choose the BW500 JSON profile,
   and click **Apply and start service**.
2. Use command port **8300** unless stock VARA already uses it. In that case
   enter **18310**. Linkwise shows the DATA port and waveform peer port. These
   are the next two ports after the command port.
3. Connect the waveform peer to the displayed port. The service accepts one
   local peer. The optional `linkwise-modemd --virtual-audio-device` bridge
   needs independent virtual input and output paths. Run it from the Windows
   desktop if the audio driver requires a logged-in session.
4. In Express, open **VARA HF P2P**, then **Settings → TNC Setup**. Enter
   **127.0.0.1** and the command port shown by the wizard. Express derives the
   DATA port. Select **500** as the maximum bandwidth and clear
   **Automatically launch TNC when session is opened**. Reopen the session
   after saving.
5. When Express shows **Ready**, enter the isolated peer's callsign and a
   center frequency for the bench session. Queue a P2P message with a
   synthetic attachment. Check the recipient's attachment SHA-256 and
   verify both session logs report a disconnect.

If Express cannot connect, check the address and command port, then open
**Windows Logs → Application** in Event Viewer and filter for
`LinkwiseModem`. If Express connects but never reaches Ready, check that the
waveform peer is attached and that the callsigns and bandwidth agree.
