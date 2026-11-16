# VarAC direct chat bench and wire reference

The target is connected text between the Linkwise Rust terminal client and
stock VarAC with VARA HF BW500. There are two separate measurements here:
application bytes at the modem sockets, and delivery across an audio path.

## Application baseline

On November 15, 2026, two copies of stock VarAC 16.1.8 ran in an interactive
Windows 11 ARM64 UTM session on the Mac mini. Both clients used independently
configured profiles, with KISS, CAT, rig PTT, automatic beacons, and reporting
disabled. Outbound Internet access was blocked for the copied clients. The
executable SHA-256 was
`9636401aae065f141ece2e3582ba0a5c4e24defc9086601fa6c2a3fc1a3ff86d`.

A loopback host relay captured the unmodified application bytes and supplied
VARA-shaped command events. No audio modem or RF was in this particular
exchange. The test observed a user-initiated `CONNECT KN6MQS BB2BB`, text in
both directions on one connection, and a user-initiated `DISCONNECT`.
The relay forwarded the following exact DATA bytes to the opposite client.
The BB2BB VarAC datastream displayed the received line and its sent reply.

| Sender | DATA bytes shown as ASCII | Context |
| --- | --- | --- |
| BB2BB | `3 <Q>` | Received after a call on the configured VarAC calling frequency. |
| BB2BB | `15 de BB2BB <S:VC>` | Startup on the regular bench frequency. |
| Both | `4 &IT&` | Typing indicator, before the text record. |
| KN6MQS | `30 Alpha confirms stock text 123.` | Text at 22:51:00 UTC. |
| BB2BB | `30 Bravo confirms stock text 456.` | Reply at 22:51:03 UTC. |

To reproduce the application trace, use separate INI profiles as in the
[VarAC multiple-instance guide](https://www.varac-hamradio.com/post/varac-cluster-multiple-instances-quick-configuration-guide).
Give each copied client a separate modem command port and following DATA
port, for example 18400/18401 and 18410/18411. Start packet logging before
launching the applications. Set different test callsigns, enable incoming
links, and select a regular frequency. Keep the route inside the isolated
bench. Dial from one client, send known text both ways, close the call,
and record both the command and DATA streams. To reproduce the relay-only
variant, respond to `VERSION` with `VERSION VARA HF v4.9.1`, acknowledge
startup commands with `OK`, and supply both clients with
`CONNECTED CALLER CALLED 500` and `DISCONNECTED` for call and teardown.
Command messages use CR termination. Forward DATA verbatim while connected
and report simulated buffer occupancy on the command socket.

## Connected DATA format

For the observed text records, send the decimal byte count in ASCII, one
ASCII space, and that many body bytes. There is no trailing separator.
The next byte after the body, if present, begins the next length. For example,

~~~text
30 Alpha confirms stock text 123.30 Bravo confirms stock text 456.
~~~

The parser counts bytes across DATA reads and accepts multiple records in
one read. The transmit limit of 1,024 bytes and the parse limit of 65,535
bytes are Linkwise limits. Stock evidence in this run covers ASCII. Linkwise
encodes outgoing text as UTF-8, renders invalid UTF-8 with a replacement
character, neutralizes terminal control characters, and caps the displayed
portion of incoming records at 4,096 characters. It discards the
observed typing indicator. Other well-framed records are shown as text.
The client implements no QSY exchange or optional service negotiations.

At attachment, the Linkwise client sends `MYCALL`, `P2P SESSION`, `BW500`,
and `LISTEN OFF` to its local daemon. `CONNECTED` carries the caller,
called station, and bandwidth in that order for both roles. Buffer notices
track local transmit bytes, and a drain is recognized when acknowledged
decreases account for the full submitted record and the buffer reaches zero.
Dropping either local socket releases the exclusive modem attachment.

## Verification

From the repository root, run the [radio-free checkout](../../rust/docs/chat.md#radio-free-checkout).
The terminal regression uses the existing virtual BW500 waveform API and
a pinned profile with SHA-256
`7fe1eed44ca50f20cb7f64e1e70b9e21d8fe9c348a920609b9888bfdde82bb48`.

| Path | Evidence |
| --- | --- |
| Stock VarAC ↔ stock VarAC, relayed DATA | Both directions captured and forwarded, with disconnect. |
| Rust chat ↔ Rust chat, Linkwise waveform sockets | Short and multi-delivery text displayed in the receiving terminals. Both transmit queues drained and both terminals restored settings on exit. Reconnected with the other terminal as caller and delivered a fresh line while draining the link. |
| Linkwise Winlink P2P regression | Exact envelope hashes in both directions and clean disconnect. |
| Stock VarAC ↔ stock VarAC, stock VARA HF audio | Pending. The one-guest relay baseline established framing. |
| Rust chat + Linkwise ↔ stock VarAC + stock VARA HF | Two Linkwise-originated text round trips passed with a follow-up line to advance the turn. See below for teardown and stock-originated results. |
| macOS ARM64 package payload | Built and expanded the package, ran its chat help and PTY waveform regression, and linted the packaged manual. |
| On-air receive | Two CRC-valid BW500 Level 3 DATA frames from live KiwiSDR streams. See below. |
| On-air connected chat and other installed platforms | Pending. |

## On-air receive

On November 15, 2026 UTC, Linkwise received two BW500 Level 3 DATA
frames while monitoring 7102.0 kHz USB through public KiwiSDRs. Receiver
audio was converted to 48 kHz mono signed 16-bit PCM and streamed to
`linkwise receive-pcm`. These were independent receive-only observations.

| Receiver | Stream offset (s) | Control | Wire CRC | Decoded content |
| --- | ---: | --- | --- | --- |
| Rockport, Maine | 37.056–41.877 | `42` | `03AC` | Closing DATA, with a plain-text fragment |
| Ontario, Canada | 95.931–100.752 | `29` | `8822` | Continuing DATA, with a compressed text fragment |

Each decoded frame contained 34 payload bytes, one control byte, and
two CRC bytes after demodulation and FEC. The Rockport payload contained
20 ASCII bytes, a closing delimiter, a caller-key byte, 11 zero pad bytes,
and the next receive level. For Ontario, Linkwise verified the frame and
the [hfmodem reference decoder](https://github.com/sstjohn/hfmodem/blob/952f79245240132f4af209afbad1f4b2ba256613/packages/hfmodem/hfmodem/kestrel/vara/compression.py)
expanded its text. Neither observation established a complete chat
transcript or an on-air Linkwise connection.

The field demo uses the captured audio, a GNU Radio spectrogram and RMS
envelope, and TikZ-DSP diagrams with byte offsets, CRC coverage, and
payload interpretation. The rolloff in the demo includes receiver
filtering. See [the CLI guide](../../rust/docs/chat.md) for the live PCM
pipeline.

## Mixed audio procedure

Use the [VARA HF interoperability audio route](vara-interoperability.md)
with one Windows stock endpoint and a local Linkwise daemon. The Windows
bridge captures the stock modem's Speakers output and plays Linkwise
waveforms into VB-CABLE. Connect VarAC to the stock command and DATA
sockets through the logging proxy. Mute VarAC sounds and narration, and
keep other applications and replay sources off the audio route. VarAC
alert sounds and a simultaneous replay both contaminated initial runs.
Start the socket log before VarAC startup and retain timestamped host
commands, DATA, and both audio directions. Capture the displayed text on
both applications and repeat after a clean disconnect.

With VarAC commanding `CHAT ON`, VARA HF 4.9.1 returned a 16-symbol
BW500 connect response to the Linkwise call. It kept the first preamble
tone and the complete 15-symbol keyed tail from the 23-symbol response.
The quiet capture for called station BB2BB contained, in order:

~~~text
62 64 62 76 64 56 76 58 54 54 74 74 60 74 70 62
~~~

These are 2048-point FFT bins at 48 kHz. The source burst was 40,320
mono float32 samples including silence, with SHA-256
`5a4ed88437f8694a854a6cd325b96b944867dcced7cd4e7ca76df5cb0bddd8fd`.
The short-response regression checks the complete keyed sequence and
rejects a different callsign.

Stock VARA's next setup acknowledgment used the same caller-keyed sequence
as the existing BW500 poll acknowledgment. The single-burst capture for
caller KN6MQS contained 30,240 samples including silence and had SHA-256
`a098f953ff3b7e1ccfe06f7acae8bfb85a8784433cb477d3bc504dcf39ee5495`.
Linkwise also accepts this complete ACK during BW500 setup and the
immediate post-confirmation phase.

### Observed text round trip

With Linkwise calling BB2BB, the stock application displayed
`Linkwise mixed message 789.` and the stock modem acknowledged the
transmission. Linkwise received the automatic VarAC sign-on. The
stock operator's `Bravo confirms mixed text 101.` stayed queued after
the sign-on turn ended. Sending `Linkwise follow-up.` opened another
exchange. VarAC received that line and Linkwise displayed the exact
operator reply. This was real modem audio over an isolated virtual
cable. A second run reproduced the same round trip. Linkwise reported
local disconnect in both runs. On the second run, the bridge stayed up
for 20 seconds after the Rust terminal closed, but no stock
`DISCONNECTED` notice arrived in that window. An immediate disconnect
after receiving the reply remains unqualified on the stock side.
The three-run repeatability gate and mixed-peer reconnect remain open.

| Event | Seconds from bench start |
| --- | ---: |
| Linkwise reports connected | 33 |
| Stock DATA delivers `Linkwise mixed message 789.` | 44 |
| Stock finishes its automatic sign-on and hands back the turn | 59 |
| Stock application submits `Bravo confirms mixed text 101.` | After sign-on drain |
| Stock DATA delivers `Linkwise follow-up.` | 75 |
| Rust terminal displays `Bravo confirms mixed text 101.` | 87 |
| Linkwise reports local disconnect | 88 |

With stock VARA originating the call, an explicit `CONNECT` command at
its host socket established BW500 and both applications reported
connected. The experiment drove the dial at the modem socket because
the automated VarAC dial controls did not issue `CONNECT`. Sending
from Linkwise immediately led to crossed traffic and the stock modem
did not acknowledge that DATA. In a second run, Linkwise stayed idle
and answered repeated stock polls. Stock keyboard DATA remained unobserved. The stock-originated
call therefore qualifies establishment only.
