# Linkwise VarAC chat implementation plan

Status: Proposed. First release target: direct, connected keyboard chat over
VARA HF BW500. The Linkwise station runs a Rust terminal client and
`linkwise-modemd`. The remote station runs stock VarAC and stock VARA HF.

## Operator experience

Proposed entry point: `linkwise chat <local-callsign>`. Reuse the CLI's
paired-port defaults (8300/8301), callsign validation, and option conventions.
The status window shows modem attachment, listening state, current peer,
link state, and pending output. Numbered callsign windows retain bounded
scrollback and show unread activity. Only one RF session is active on the
modem at a time. The operator can read an old window while a different
conversation is active.

| Input | Behavior |
| --- | --- |
| `/connect CALL` | Call the named station and switch to its window after connection. |
| `/listen on\|off` | Enable or disable incoming calls explicitly. An accepted call activates the corresponding peer session. |
| Text and Enter | Queue one message for the active peer in that peer's window. Keep the draft if the queue is full or the session is unavailable. |
| `/window N` or Alt+number | Switch between status and peer windows. Provide command access when the terminal intercepts Alt. |
| Up/Down, Page Up/Down | Recall local input and navigate scrollback. |
| `/status`, `/help [command]` | Inspect the link and find command syntax and key bindings. |
| `/disconnect`, `/quit` | Drain within a documented deadline, close the session, and restore the terminal on exit. |

Start with in-memory history. Define the input escape for sending a literal
leading slash. Keep connection and listen actions explicit. Schedule
discovery, beacons, CQ, broadcasts, VMail, file transfer, QSY, and multi-station
features separately after direct chat qualifies. Borrow the interaction
patterns from [irssi's user guide](https://irssi.org/New-users/) and
[command reference](https://irssi.org/documentation/help/).

## Design

- **Terminal UI.** Add a chat subcommand at the existing CLI dispatch boundary.
  Keep terminal rendering and key handling separate from the chat session.
  Use an event loop that stays responsive during RF turnaround and queue drain.
  Restore terminal modes on normal exit, errors, and handled signals.
- **Chat session.** Model idle, listening, dialing, connected, and draining
  with explicit socket-failure transitions. Bind drafts and queued writes to
  the current peer and session. Clear or fail pending writes on session loss
  and require an explicit resend. Distinguish local queueing, confirmed
  transport progress where the modem exposes it, and unknown delivery after
  interruption. The stock client transcript is the qualification evidence
  that a message appeared to the recipient.
- **VarAC codec.** Incrementally parse the modem DATA byte stream across
  arbitrary TCP read boundaries. Establish startup exchange, framing,
  encoding, delimiters, and required control replies from documentation
  or stock-peer observations. Preserve text supported by the tested peer,
  apply evidence-based size limits, and handle bounded malformed or unknown
  records. Render received content as text with terminal control sequences
  neutralized. Only implement automatic responses shown necessary by the
  stock interoperability evidence.
- **Modem connection.** Use the existing command and DATA sockets with
  Linkwise owning the waveform, ARQ, audio, and station control. Review the
  CLI mail and send paths for reusable connection, event, timeout, and
  shutdown code. Keep the chat protocol independent of Winlink B2F.
  The modem currently serves one paired host. Chat and a live mail RF
  exchange therefore take turns, and chat must report an occupied modem
  clearly. SMTP/IMAP mailbox service is a separate process from RF exchange.
  Regress the existing mail path whenever shared modem behavior changes.

## Milestones and acceptance evidence

| Milestone | Work and evidence required to exit |
| --- | --- |
| 1. Establish the stock baseline | On isolated Windows audio loopback, start with VarAC 16.1.8 and VARA HF 4.9.1, the versions in the prior checkpoint. Record exact versions and settings for the run. Resolve the prior destination-entry issue by verifying the field and an outgoing `CONNECT` in the command log. Complete a stock-to-stock call in each direction, exchange text both ways, disconnect, and call again. Capture time-correlated command/DATA bytes and both client transcripts. Document what byte sequences and host commands were required, which behavior is optional, and any open protocol questions. Use the [official VarAC quick start](https://www.varac-hamradio.com/post/varac-quick-start-guide) for operator setup. |
| 2. Implement and exercise the Rust core | Turn observed exchanges into small reproducible codec fixtures. Cover split and combined reads, empty and oversized input, supported character encoding, unknown control records, stalled writes, disconnect during send, and socket loss. Replay the stock capture against the same session code used by the CLI. Exchange text over two Linkwise daemons on the existing virtual BW500 waveform bench before introducing the stock peer. |
| 3. Build the terminal workflow | Implement the status and callsign windows, input history, scrollback, and help from the command table. Exercise it in a real terminal or PTY, including resize, window switching during inbound traffic, a busy modem, Ctrl-C during a call, and clean terminal restoration after exit. Measure responsiveness during a blocked send and verify that drafting and scrolling remain usable. |
| 4. Qualify mixed peers | Run Linkwise client + Linkwise modem against stock VarAC + stock VARA HF on the same isolated audio route as the stock control. Complete three consecutive runs in each call direction. In each run, exchange a short line and a message spanning multiple modem deliveries in both directions within the same call, compare both transcripts, verify one display per message, disconnect to idle, and reconnect without restarting either modem. Exercise at least one lost-link or abort case and confirm pending output is visibly resolved with no automatic replay. Rerun the existing Winlink P2P regression when shared transport code changes and verify mail can acquire the modem after chat exits. |
| 5. Ship documentation and package checks | From an installed build on each platform advertised for chat, run help, open the packaged manual where supported, follow the quick start, and record results. Publish tested peer versions, platform coverage, audio path, configuration, pass counts, and open limitations. Treat an authorized on-air QSO as a separate qualification result with its own evidence. |

Use the checkpoint `22d1561` on `origin/codex/varac-validation` as prior bench
evidence. That run captured stock VarAC startup commands and isolated a
KISS startup issue, but its direct-call attempt produced no outgoing
`CONNECT`. Its host compatibility changes should be evaluated individually
if needed. The primary mixed-peer test has stock VarAC attached to stock
VARA locally, so the Rust client can use the Linkwise host interface directly.
The [modem interoperability report](../testing/vara-interoperability.md)
and [lab notes](../../lab-notes/lab-notes.tex) cover separate byte-transfer
and receive-only beacon evidence. Keep radio-free runs isolated with CAT,
PTT, automatic transmissions, and external reporting disabled.

## Documentation release gate

Match the existing [Linkwise manual](../man/linkwise.1) and CLI regression
coverage. Land the following with the corresponding functionality.

| Deliverable | Review criterion |
| --- | --- |
| `linkwise chat --help` and in-app `/help` | Operators can locate callsign, port options, defaults, commands, key bindings, and shutdown behavior from the installed program. |
| `man linkwise` | Add the chat synopsis, settings and defaults, examples, errors and exit behavior, and any transcript storage behavior. Verify installed help and manual against the implemented flags and defaults. |
| Quick start linked from the Rust README | Walk through modem startup, attaching the client, originating and receiving a call, sending and reading text, disconnecting, and handing the modem back to mail. Include a radio-free walkthrough and troubleshooting for socket pairing, modem occupancy, mismatched callsign, timeouts, and link loss. |
| Developer and protocol reference | Document the command/DATA boundary, session transitions, framing and encoding with annotated examples, limits, timer behavior, and source or fixture provenance for each required wire behavior. |
| Compatibility report and bench runbook | Provide reproducible commands and settings, expected observations, test versions and hashes where applicable, the run matrix, and transcript/log locations. Identify which results came from replay, waveform simulation, mixed-peer audio, or on-air operation. |

Implementation is complete when the mixed-peer gate and installed
documentation checks pass for the advertised targets. Record pending or
failed cells in the compatibility report with their observed failure.
