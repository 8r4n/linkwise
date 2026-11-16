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

## Verify a station identity

Start both Linkwise clients with `--pki FILE` for certificate-authenticated
chat. Select encryption with `--encrypt` when permitted. The
[shared PKI guide](pki.md) covers station credentials, protected radio mail
and exporting a message with `/proof PATH`. Enroll a peer over
[`/cert`](pki.md#exchange-certificates-in-chat) or from a
[QSL postcard](pki.md#mail-a-qsl-card-with-public-certificates).
Use `/encrypt` on both terminals to upgrade an active verified call. Chat
holds new text as a draft until the upgrade completes. To require hybrid
key exchange set the [same PQC profile](pki.md#require-hybrid-post-quantum-chat)
on both stations.

## Terminal controls

| Command or key | Effect |
| --- | --- |
| `/status` | Show the link and listen state in the current window. |
| `/who` | Show your callsign and the current peer. In the Status window, show the active call. |
| `/window N` or Alt+0–9 | Select a numbered window. Window 0 is status. An asterisk indicates activity in another window. |
| Up/Down | Recall recent input. |
| Page Up/Down | Navigate the current window's scrollback. |
| Backspace or Ctrl-U | Delete the last character or clear the draft. |
| `//text` | Send a literal leading slash. |
| `/help` | Show commands and key bindings. |

Each window has its own draft and bounded scrollback. Chat keeps up to nine
peer windows and 200 records per window in memory. A new peer evicts the
oldest retained peer window once the limit is reached. Messages wrap at word
boundaries and reflow when you resize the terminal. Long words and URLs
can span several rows.

Chat writes a signed message to disk when you request a proof export. A
message can contain up to 1,024 UTF-8 bytes. The CLI sends one line at a time
and keeps a draft while the modem is still sending the previous line. Review
a draft in its peer window before sending it. The client never replays an
interrupted line.

## Presence

The peer and Status windows show local-time notices when a station joins,
leaves, disconnects, or hits a call timeout. A PKI peer joins after
verification. A locally requested orderly disconnect is shown as a leave.
An unqualified modem disconnect is shown as a disconnect. The client uses
connection notices and existing call deadlines, so a quiet peer stays
connected. Each roster covers one direct BW500 call.

## Transfer a file

Select the connected peer window and enter `/sendfile PATH`. The peer
reviews the offer and enters `/accept 1` to save under the offered basename
in the directory from which chat was started. Use `/accept 1 PATH` to
save elsewhere or `/reject 1` to decline. Paths can contain spaces. Only
one plain file transfer can run per connection. Each file is limited to
32 KiB at the sender and after expansion, with a small allowance
for gzip overhead on the wire.

Linkwise compresses the file with gzip and verifies SHA-256 over the
compressed bytes before publishing the received file. The received file is
published only if the chosen path is free. `/transfers` shows progress.
The sender reports delivery only after the peer sends its application
receipt. If the receipt is lost, the peer may already have the file.
`/cancel 1` stops queued file writes and notifies the peer at a record
boundary. If cancellation would leave a partial record, the client
aborts the call. The client does not automatically resume or resend.

All file transfers require a build with the secure-transfer feature.
Linkwise checks the AWS-LC provider's FIPS mode before hashing.
Plain SHA-256 checks file integrity without authenticating the peer.
Stock VarAC 16.1.8 advertises MD5 for files, so Linkwise declines those
offers. Text chat with stock VarAC is still available.

## Use an encrypted file session

Build with the `secure-transfer` feature and follow the
[secure transfer guide](secure-transfer.md) to configure certificates
and peer pins on both stations. Each chat configuration must use the
same local callsign and ports as the chat instance. Start both clients
with `linkwise chat --secure-config FILE LOCAL [COMMAND_PORT DATA_PORT]`.

The receiver enters `/receivefile --encrypt PATH` first and chooses the
destination. The sender enters `/sendfile --encrypt PATH`. Each chat
client uses its modem attachment for a dedicated mutually authenticated
TLS 1.3 session. An active chat call closes before the transfer starts,
and the chat terminal returns after the transfer. Reconnect to continue the conversation.
While encrypted transfer is running, Ctrl-C interrupts it.

The secure session requires the AWS-LC FIPS provider and authenticates
the configured peer before sending the gzip stream. The receiver verifies the authenticated SHA-256 digest,
enforces the configured size limit after decompression, and publishes
without overwriting. The filename is selected locally by each operator.
Errors stop the transfer without trying plaintext. Both peers need the
Linkwise chat encryption feature. Deployment validation depends on the
module version and operating environment described in the secure transfer
guide. Use encryption on virtual audio or
another transport where it is authorized. U.S. amateur-radio rules
generally prohibit transmitting messages encoded to obscure their meaning.
[47 CFR § 97.113(a)(4)](https://www.ecfr.gov/current/title-47/chapter-I/subchapter-D/part-97/subpart-B/section-97.113)

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
| Disconnect takes too long | The client allows 120 seconds without transmit acknowledgment progress while draining, then 30 seconds for the modem to disconnect. It requests an abort if either deadline expires. |

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
