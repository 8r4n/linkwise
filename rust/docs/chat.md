# Direct chat over Linkwise BW500

`linkwise chat` is an interactive Rust terminal client for direct, connected
VARA HF BW500 text. See the [qualification report](../../docs/testing/varac-chat.md)
for the current stock VarAC and waveform results.

## Start a station

Use the [station setup guide](modem-setup.md) to configure BW500 and obtain
the measurement profile. Start the daemon and the client in separate terminals
on the same computer. Use the callsign from your station configuration.

~~~sh
linkwise-modemd --serve-station station.json
linkwise chat YOURCALL
~~~

The modem starts with listening off in chat. To receive a direct call, enter
`/listen on`. To originate one, enter `/connect PEERCALL`. Once the peer
window opens, type a line and press Enter. The status line shows the active
peer and whether an outgoing line is pending. The modem acknowledgment
confirms transport progress. It cannot confirm what the other operator sees.

Use `/disconnect` to end the call or `/quit` (also Ctrl-C) to close the
session and release the modem. When coordinating a direct bench call with
VarAC, use BW500 and select a regular operating frequency in VarAC to avoid
its calling-frequency QSY exchange. Use virtual audio with CAT, PTT, CQ,
beacons, and reporting disabled during a radio-free test.

## Terminal controls

| Command or key | Effect |
| --- | --- |
| `/status` | Show the link and listen state in the current window. |
| `/window N` or Alt+0–9 | Select a numbered window. Window 0 is status. An asterisk indicates activity in another window. |
| Up/Down | Recall recent input. |
| Page Up/Down | Navigate the current window's scrollback. |
| Backspace or Ctrl-U | Delete the last character or clear the draft. |
| `//text` | Send a literal leading slash. |
| `/help` | Show commands and key bindings. |

Each window has its own draft and bounded scrollback. Chat keeps up to nine
peer windows and 200 records per window in memory. A new peer evicts the
oldest retained peer window once the limit is reached. There is no transcript
file. A line can contain up to 1,024 UTF-8 bytes. The CLI sends one line at a
time and keeps a draft when the modem is still sending the previous line.
Review a draft in its peer window before sending it. The client never
replays an interrupted line.

## Radio-free checkout

From the repository root, build the client and modem, fetch the verified
profile, and run the two-station terminal regression. It uses PTYs and two
Linkwise daemons wired together by their virtual audio sockets.

~~~sh
cargo build --locked --manifest-path rust/Cargo.toml --features audio-device --bins
profile=$(python3 tools/fetch-modem-profile.py)
python3 tools/test-varac-chat.py rust/target/debug/linkwise rust/target/debug/linkwise-modemd "$profile"
~~~

This exercises the same terminal program and BW500 modem code used by the
client workflow. The bench sends short text and a reply spanning more than
one modem delivery and checks acknowledgment, disconnect, and terminal restore.

## Troubleshoot

| Observation | Action |
| --- | --- |
| Modem setup fails | Confirm the callsign, BW500 mode, and both command and DATA ports. If overriding the defaults, pass both ports as `linkwise chat YOURCALL COMMAND_PORT DATA_PORT`. |
| Modem is occupied | Exit the existing chat or mail RF client. One client holds the paired local ports at a time. SMTP and IMAP services can run independently of that RF session. |
| Call times out | Check the peer's listen setting, the audio route, and the station configurations. The client aborts a call after 90 seconds. |
| Stock VarAC reply stays queued after sign-on | In the tested Linkwise-originated call, a follow-up text line opened a new transmit turn and allowed the queued reply to arrive. |
| Pending output followed by link loss | Delivery is unconfirmed. Reconnect and resend manually if appropriate. |
| Disconnect takes too long | The client allows up to 30 seconds for drain and disconnection, then requests an abort. |

## Stream audio from a KiwiSDR

For a receive-only check, pipe a public receiver's audio into Linkwise. This
example uses the KiwiSDR project's `kiwirecorder.py` and FFmpeg. Use an open
receiver with a free slot. Set the frequency in kHz and keep a separate
decoder process for each continuous connection.

~~~sh
python3 kiwirecorder.py --nc --nc-wav -s kiwisdr.kfsdr.com -p 8075 \
  -f 14105.750 -m usb -L 100 -H 3000 --tlimit 60 -q -u Linkwise-RX |
  ffmpeg -hide_banner -loglevel warning -f wav -i pipe:0 \
    -ac 1 -ar 48000 -f s16le pipe:1 |
  linkwise receive-pcm
~~~

`receive-pcm` emits CRC-checked BW500 frames as JSON with sample positions,
control fields, and payload hex. Passive conversation reconstruction needs
session context, DATA reassembly, and decoding of compressed text where used.
The interactive `linkwise chat` path gets application bytes from the modem's
connected DATA socket.
