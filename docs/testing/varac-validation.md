# VarAC HF BW500 qualification

Use two isolated Windows desktops with distinct test callsigns. Record VarAC and
modem versions, Linkwise commit and installer hash, and the BW500 profile hash.
Use the [official VarAC download](https://www.varac-hamradio.com/download) and
[configuration guide](https://www.varac-hamradio.com/post/varac-quick-start-guide).
Disable CAT, PTT, CQ, beacons, and online reporting for the radio-free run.

## Linkwise waveform bench

Build the modem and exercise the command, DATA, and waveform sockets with the
existing regression harness. Supply the locally configured BW500 profile.

```sh
cargo build --locked --manifest-path rust/Cargo.toml --features audio-device --bin linkwise-modemd
python3 tools/test-virtual-bw500.py rust/target/debug/linkwise-modemd "$BW500_PROFILE"
```

## Client runs

Use the installed Windows service in radio-free waveform mode and pair it with
a second Linkwise station. Follow the waveform setup in the
[Windows installation guide](../install/windows.md). Configure each VarAC
instance in VARA HF mode with the local Linkwise command port. Check the DATA
connection on the following port and verify the running modem process. Record
whether the tested VarAC version allows attaching with auto-launch disabled.
Capture the startup commands and responses to diagnose a rejected attachment.

| Pairing | Initiator | Runs |
| --- | --- | ---: |
| Linkwise / Linkwise | Each side | 3 per side |
| Stock VARA HF / stock VARA HF | Each side | 1 per side |
| Linkwise / stock VARA HF | Each side | 3 per side |

For the stock and mixed runs, route audio through the isolated virtual audio
bench. Use the same route for the stock control and mixed runs.

In each run, compare known short messages and one longer message spanning
multiple frames in both directions. Record the sent and received text and note
any application normalization explicitly. Both clients must show idle after
disconnect, and a fresh connection must work without restarting either modem.
Retain client transcripts and time-correlated modem logs. Record results by
pairing and initiator. Fix Linkwise defects with a regression case and rerun
the affected pairing and Winlink P2P checks where the modem behavior is shared.

Use completed client runs to publish exact settings, tested versions, and
limitations on the VarAC compatibility page.

## Windows bench status

The VarAC 16.1.8 ZIP linked from the [release announcement](https://www.varac-hamradio.com/post/varac-v16-1-8-is-here)
was staged on both Windows ARM64 VMs. Both guest copies matched SHA-256
`e543116ade4b7c920f52869d4509d31a702634eef341f68f37e0612da38241b7`.
VarAC started and initialized its database on both desktops.

A TCP probe connected to the installed Linkwise service on command port
18310, DATA port 18311, and waveform port 18312. The service returned
`VERSION LINKWISE VIRTUAL BW500` and accepted `LISTEN ON` and
`LISTEN OFF`. This probe used the command socket directly.
VarAC configuration, message transfer, and disconnect validation are pending
desktop access. The VARA Peer VM was at the Windows lock screen.

## VarAC 16.1.8 startup against Linkwise

On the peer desktop, select **Settings → RIG Control and VARA
Configurations → Vara**. Select **VaraHF**, set the host to `127.0.0.1`
and the main port to `18310`, clear **Start modem upon startup**, and
enable **Log VARA commands**. Save and restart VarAC.

VarAC opened Linkwise command and DATA sockets with auto-launch disabled.
It then logged a modem type mismatch (configured `VaraHF`, detected
`VIRTUAL`) after receiving `VERSION LINKWISE VIRTUAL BW500`.
VarAC also logged a KISS connection error on the default port 8100.
Its startup trace included `BW500`, `CHAT ON`, `IGNOREKISSDCD ON`,
`LISTEN CQ`, `VERSION`, `MYCALL W1AW W1AW-T`, and `LISTEN ON`,
with several `WRONG` replies. Message transfer and disconnect remain
unverified. The primary VarAC client was still on its first-run form.

## Host compatibility changes

Stock VARA HF 4.9.1 returned `VERSION VARA HF v4.9.1` and accepted
each captured startup command when sent individually. VarAC 16.1.8
requires at least 4.8.8 on the host interface. Linkwise now returns
`VERSION VARA HF v4.8.8 Linkwise` on the waveform and native HF host
interfaces. The installed ARM64 service returned that exact response
in a direct TCP probe.

Linkwise accepts the configured callsign followed by its `-T` companion
in `MYCALL`. `CHAT ON` enables incoming ARQ calls. Chat-specific timing,
CQ reception (`LISTEN CQ`), and KISS transport remain unimplemented.
`IGNOREKISSDCD` is accepted because the current backend has no KISS
transmitter to gate. The VarAC peer retest still logged a KISS connection
error. Complete the two-client transfer before qualifying the setup.

The ARM64 Inno Setup installer tested on the primary guest had SHA-256
`4a35bdf9f8d8a7825dd7600599ccec2a8e2597d9eb4d744d77d754dff43818c4`.

## KISS startup isolation

A temporary loopback listener on port 8100 accepted the VarAC 16.1.8
connection without exchanging any KISS bytes during startup. With it
present, VarAC reached modem initialization without the previous KISS
and non-connected socket exceptions. Windows reported command, DATA,
and KISS established under the same VarAC process. After the temporary
listener closed, KISS moved to CLOSE_WAIT while the command and DATA
connections stayed established. The listener was a diagnostic probe
and did not implement KISS frames. The chat transfer test still
requires desktop access.

## Radio-free test profiles

The primary client and installed service use `AA1AA`. The peer client
and foreground modem use `BB2BB`. Both VarAC profiles use the dummy
locator `AA00AA`, with reporting and automatic beacons disabled.

With VarAC stopped, set `VarahfEnableKissInterface=OFF` in the active
`VarAC.ini`, then restart VarAC. In these VMs the active file was
`C:\Windows\SysWOW64\VarAC.ini`. Both clients connected to their
Linkwise command and DATA ports with KISS disabled. The peer command log
confirmed that Linkwise accepted `MYCALL BB2BB BB2BB-T` and still
returned `WRONG` for `LISTEN CQ`. The two-client chat and disconnect
checks remain pending.

## Stock VARA direct-call control

The peer VM's VarAC 16.1.8 client attached to stock VARA HF 4.9.1 on
port 8300 using `BB2BB`, BW500, and KISS disabled. The startup trace
recorded `BUSY ON` and `BUSY OFF` notifications and successful replies
for the startup commands, including `LISTEN CQ`. The Mac host locked
before the direct call to `AA1AA` could be attempted.

After the Mac was unlocked, the stock control showed the same symptom
as the Linkwise run. The destination field stayed empty during the
attempt to enter `AA1AA`, and the stock command log showed no outgoing
`CONNECT`. The run did not establish whether VarAC rejected the input
or the UTM desktop interaction failed to deliver it to the field.
Resolve the destination entry on stock VARA before using the direct
call as a comparison of modem behavior.
