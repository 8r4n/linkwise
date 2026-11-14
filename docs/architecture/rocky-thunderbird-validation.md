# Rocky production server and Thunderbird qualification

The client requirement is Thunderbird on Windows, macOS, Linux, Android
and iOS. Rocky hosts the Linkwise mail and modem services. The operational
HF transport and the local client IP network are separate paths. Define
the allowed client subnet, access policy, and maintenance topology before
claiming host-level DNS, SMTP or other IP controls inapplicable. TCP
submission and IMAP are now production interfaces requiring assessment.

## Existing RPM and bench result

The assessed Rocky ARM64 RPM contains the receive-only station candidate.
Its installed binary exits 2 on the authenticated SMTP STARTTLS probe,
reporting that TLS requires `mail-tls`. The embedded SMTP and IMAP servers
bind to IPv4 loopback. The current production package therefore lacks the
authenticated TLS client interface and network exposure required by this
deployment. A separate ARM64 `mail-tls` build was tested on the same Rocky
guest using a temporary SSH tunnel and bench CA.

| Client | Evidence against Rocky |
| --- | --- |
| iOS | Thunderbird's SMTP, IMAP and MIME libraries ran on an iPhone 17 simulator. Submitted over STARTTLS, retrieved over validated implicit IMAP TLS, matched attachment SHA-256 `5af8f01f9fb8310f3815941a0629b40b45dd0b15f380640d02fc3f69073efd8b`, and logged out. Both Rocky virtual BW500 exchange processes exited cleanly. The stock attachment UI was outside this test. |
| Windows 11 ARM64 | Guest booted and the installer was staged. The guest agent did not yield a verified Thunderbird installation or client exchange. |
| macOS | Thunderbird 153.7.0 launched with a dedicated profile. No attachment submission reached the Rocky bench before its timeout. |
| Linux | Thunderbird 159.0 started on Ubuntu 24.04 with a test profile and bench CA. Initial run was invalidated by a listener port collision. A fresh server run reached protocol preflight, with no verified Thunderbird submission or attachment retrieval. |
| Android | Thunderbird 25.0 on API 36 completed SMTP and IMAP setup after accepting the fingerprint-verified bench certificate exception. The attachment message remained in Outbox on retry and the Rocky mailbox received no queued submission before timeout. The client also reported no Sent folder. |

The Rocky protocol preflight rejected plaintext authentication, bad
credentials, invalid envelope sender, oversized and interrupted submission,
untrusted CA and hostname mismatch. Evidence files are in the private
`~/Virtual Machines/Linkwise Rocky9 Evidence/mail-bench` directory.

## Reproduction

Build with `tools/rocky9-vm/build-mail-bench.sh ABSOLUTE_OUTPUT_DIR` using
the local Rocky ARM64 builder image or run the recorded Cargo build in an
equivalent Rocky build container. Use a disposable guest and register the
exact test executables and Python scripts with application control when
enabled. Follow the certificate, trust and simulator instructions in the
[Thunderbird iOS bench](../../tools/ios-thunderbird/README.md). Stage the
bench scripts and BW500 profile using the repository's relative layout.
Run `bench.py` on Rocky with `--binaries` pointing to the Rocky build.
Keep the listeners on loopback and forward TCP 12525 and 11143 to the
client's loopback for the simulator test. Use a new mailbox directory and
Xcode result bundle for each run.

## Production release gates

- Provide an installable Rocky mail service with explicit client interfaces,
  durable mailbox state, authentication, TLS certificate lifecycle and
  a defined forwarding trigger. Use a TLS implementation and operational
  environment with substantiated CMVP coverage where FIPS applies. The
  current bench `mail-tls` build uses rustls with ring, and this run
  establishes interoperability only.
- Qualify each Thunderbird app with a unique attachment through the same
  Rocky package and HF route. Record client version and OS, certificate
  verification, submission, sender queue confirmation, exactly one incoming
  copy, saved attachment SHA-256 and clean modem disconnects. Establish
  Linux, macOS, Windows and Android app results and the iOS application UI
  result independently.
- Reassess the Security Target for production mail and transmit, the
  authorized client network, and the production TLS implementation.

## SMTP compatibility follow-up

The Rocky retest added TLS-only SMTP AUTH PLAIN alongside AUTH LOGIN.
The protocol preflight verified valid credentials and rejection of bad
credentials and mismatched authorization identities. The rebuilt Rocky
ARM64 binaries passed the iOS simulator attachment exchange with the
same SHA-256 and clean BW500 disconnects. Android's queued attachment
remained in Outbox on retry. The IMAP endpoint still exposes a read-only
INBOX, and Android's normal Sent-folder workflow remains open.

The RPM builder has an explicit `--mail-tls` bench option with a separate
release identity and notices for its enabled dependencies. A TLS-enabled
RPM was emitted before the distinct release suffix was added. Its smoke
run was interrupted when host storage filled and the container filesystem
became read-only. The final suffixed RPM and lifecycle remain unverified.

## Rocky server RPM with system OpenSSL

The server build produces `linkwise-modem-0.1.0-1.rockymail.aarch64.rpm`.
It passed container install, smoke and uninstall tests and checksum
verification. The RPM includes the two-way modem binaries and separately
configurable SMTP and IMAP systemd services. The [Rocky server guide](../install/rocky-server.md)
documents radio, client interface, TLS, and forwarding setup.

With executables extracted from this RPM on the Rocky guest, the server
rejected startup when the OpenSSL FIPS policy was disabled. Both protocols
rejected unauthenticated plaintext binds on non-loopback addresses.
Virtual BW500 passed two-way exchange with byte-identical attachments
and clean disconnects. Thunderbird iOS libraries also passed authenticated
SMTP STARTTLS and implicit IMAP TLS using system OpenSSL on Rocky. The
iOS attachment hash matched and both virtual modems disconnected cleanly.

Evidence is in `mail-bench/openssl-rpm.log`, `openssl-two-way.log`,
`openssl-ios-server.log` and the iOS result bundle in the private
Rocky evidence directory. The signed receive-only RPM remains installed on
the assessment guest. Signed server deployment, physical radio tests,
server-side Sent-folder support, remaining Thunderbird apps, and exact
CMVP coverage remain open.

## Mobile rerun with the installed RPM

The Rocky guest installed the ARM64 server RPM with a disposable bench
signature and DNF local signature verification. Both mobile runs used
the installed `/usr/bin` binaries with disposable mailboxes and loopback
TLS listeners. The production systemd mail units were inactive.

- iOS passed in the iPhone 17 simulator using Thunderbird's protocol
  libraries. Client and Rocky logs agreed on attachment SHA-256
  `5af8f01f9fb8310f3815941a0629b40b45dd0b15f380640d02fc3f69073efd8b`.
  Both virtual BW500 modems disconnected cleanly.
- Android Thunderbird 25.0 retried the attachment from Outbox against a
  fresh Rocky mailbox. It remained in Outbox and Rocky had no queued or
  delivered message, so checksum and disconnect gates could not run.

Logs and the iOS result bundle are in the private Rocky evidence directory
under `mail-bench/mobile-rerun`. The server RPM remains installed.

## Android send diagnosis

A TCP relay traced the emulator port to SMTP from the installed Rocky RPM.
The Android shell positive control received the Rocky `220` greeting,
sent `EHLO`, received the STARTTLS advertisement, and completed `QUIT`.
Thunderbird's Outbox **Send messages** retry produced no additional
connection on that relay and the message stayed queued. The observed
failure therefore precedes SMTP negotiation, authentication, queueing
and modem forwarding. An Android background job showed a timing
constraint, but its purpose was unverified. The release APK did not
permit access to its private worker state via `run-as`. Temporary
notification permission exposed no send error. The exact application
reason remains open. Trace evidence is under
`mail-bench/mobile-rerun/android-wire.log`.

### Android root cause and successful retest

Read-only inspection of the emulator's Thunderbird Outbox database
recovered the original SMTP rejection. Reconstructing the test message
from the stored MIME parts reproduced the B2F validation error. The
message's text/plain part was empty. Linkwise now maps an empty MIME
text part to a blank CRLF line for B2F, retaining the attachment bytes.

After the fix, a fresh attachment-only message sent from Thunderbird
25.0 through Rocky over SMTP STARTTLS and virtual BW500. Android
retrieved it over IMAP TLS. The received attachment extracted from
Thunderbird's Inbox matched SHA-256
`5af8f01f9fb8310f3815941a0629b40b45dd0b15f380640d02fc3f69073efd8b`.
Both virtual modem ends disconnected cleanly. The test used a
Rocky-built patched mail CLI and the installed RPM's modem.
Thunderbird used the bench certificate exception and Send and Delete
because server-side Sent storage is still unimplemented. The installed
RPM needs a rebuild to incorporate the mail importer fix.

The Rocky ARM64 RPM was rebuilt with the attachment-only fix. It
passed the package lifecycle smoke test and checksum verification.
The CLI extracted from the rebuilt RPM imported the reconstructed
Android message and preserved the exact 1,400 attachment bytes.
Evidence is in the private Rocky evidence directory under
`mail-bench/android-fixed-rpm`. This build was tested in a disposable
container and awaits installation on the Rocky guest.

## Enabled Rocky services retest

The development-signed RPM is installed. The modem, SMTP and IMAP units
are enabled and active. The modem runs in BW500 mode with ALSA loopback.
Mail uses loopback-only fixture credentials and a bench certificate.
Both mobile runs submitted and retrieved through the active SMTP and
IMAP systemd units. A one-shot test harness forwarded through two virtual
BW500 peers, then published the peer inbox copy to the live IMAP mailbox.
The native modem unit remained active during this separate virtual path.

The iOS simulator protocol test and the Android Thunderbird app each
retrieved an attachment matching SHA-256
`5af8f01f9fb8310f3815941a0629b40b45dd0b15f380640d02fc3f69073efd8b`.
Both virtual modem ends disconnected cleanly in each run. Android used
the bench certificate exception and Send and Delete. The first iOS run
needed a manual inbox publication after the forwarding helper called an
API absent in Python 3.9. The helper now uses `os.link`, which successfully
published the Android message. Evidence is under `mail-bench/services-mobile`.

## Outlook mobile private CA test

A dedicated private CA issued an RSA server certificate with DNS SAN
`outlook.linkwise.test` and IP SAN `10.0.2.2`. Isolated listeners from the
installed Rocky RPM were routed through a host relay, separate from the
running mail units. Rocky OpenSSL verified the chain and IP identity on
both listeners using TLS 1.3. No server ingress from the internet was
opened.

Outlook for Android 5.2644.1 opened IMAP connections through the
emulator host path. It reported an invalid server certificate before
installing the CA. After the CA was visible in Android's **User**
trusted credentials, a fresh manual setup with the same IP and port
again produced the certificate warning and an IMAP connection in the
relay log. No certificate exception was accepted. Outlook did not
reach verified SMTP or attachment testing. This result describes the
user-installed CA and IP-address setup tested here. A DNS-name or
managed trust deployment was outside the completed test.

The iPhone simulator accepted the test CA into its keychain. There was
no Outlook build on the simulator and no physical iPhone connected.
Direct iPhone access, disconnected-LAN operation, and attachment
verification therefore remain untested. Raw connection and TLS logs are
under `mail-bench/outlook-private-ca`. The temporary Rocky listeners and
host relay were shut down. The CA remains on the test devices.

## Gmail for Android private CA test

Gmail 2025.03.02.732962214.Release on the Play-enabled Android emulator
accepted manual IMAP and SMTP settings for the isolated listeners from
the installed Rocky RPM. Both used implicit TLS, the private CA in
Android's user trust store, and the certificate IP SAN for `10.0.2.2`.
Gmail used standard SSL/TLS validation with no certificate exception.

Gmail submitted a message with a 1,400-byte attachment. The bench
forwarded it through two virtual BW500 Linkwise modems, verified SHA-256
`5af8f01f9fb8310f3815941a0629b40b45dd0b15f380640d02fc3f69073efd8b`
at the receiving modem's mailbox, and observed a clean disconnect at
both ends. The bench published the peer mailbox copy to Rocky's IMAP
inbox. Gmail listed the subject but displayed neither the body nor the
attachment. The credential-redacted IMAP trace shows successful LOGIN,
SELECT, UID SEARCH and a UID/FLAGS fetch. Rocky rejected Gmail's STATUS
request, and the trace showed no body retrieval. Attachment SHA-256
verification from Gmail itself remains open. Evidence is under
`mail-bench/outlook-private-ca` in the Gmail-prefixed logs.

### Gmail retest with IMAP STATUS

Linkwise now handles IMAP4rev1 STATUS for the read-only INBOX using the
same mailbox snapshot as SELECT. The protocol regression checks STATUS
before and after selection, empty and populated counts, UID consistency,
Gmail's UID SEARCH and flags fetch, and exact MIME attachment retrieval.

The Rocky ARM64 RPM passed its package smoke test. The Linkwise Dev
signature was verified, and the signed RPM was reinstalled with package
signature checking enabled. The signed RPM SHA-256 is
`60f4500dfcf961f89838fd3bd3cf3d09950d6b29108e12dc3336a37c67bbbb84`.

Gmail received OK for STATUS "Inbox" and listed a fresh test message
submitted by the Linkwise CLI. Both virtual BW500 peers disconnected
cleanly and the fixture matched the original SHA-256 at the receiving
mailbox. Gmail displayed the subject without the body or attachment. The
app also issued STATUS "Sent" and then CREATE "Sent". Linkwise's read-only
IMAP server exposes only INBOX, and the new Gmail submission remained
queued. A checksum from a Gmail download and a fresh Gmail-to-Gmail
round trip are still pending. Build, signature, protocol and redacted
Gmail logs are under `mail-bench/gmail-rpm`. The temporary listeners were
stopped and the three Rocky service units were active after the test.

### Gmail retest with persistent Sent

The development-signed Rocky ARM64 RPM passed the package smoke test
and was reinstalled with signature verification enabled. Its SHA-256 is
`f7f23a5bf8d1dc1626604d3724b316f6e11cfe52adcc6107555cb642516cec62`.
The signed package exposes a pre-created Sent folder. IMAP APPEND stores
a copy in a separate append-only archive. The protocol regression covers
oversized and interrupted APPEND, attachment retrieval from Sent across
a server restart, stable Sent UIDs, and a single queued radio submission.

Gmail's previously queued message left its Outbox after the Sent folder
became available. The installed Rocky binaries forwarded the message
through two virtual BW500 peers. The receiving mailbox attachment matched
SHA-256 `5af8f01f9fb8310f3815941a0629b40b45dd0b15f380640d02fc3f69073efd8b`
and both modems disconnected cleanly. Gmail refreshed and listed this
message in Inbox, but opening it showed only headers. The redacted
trace shows successful STATUS and SELECT, then UID SEARCH and UID FETCH
for FLAGS. It shows no body or MIME section request. The Gmail test left
the server-side Sent archive empty, so actual Gmail APPEND behavior and a
Gmail-downloaded checksum remain open. Evidence is under
`mail-bench/gmail-sent-rpm`.

### Gmail attachment round trip with MIME section retrieval

A fresh Android Gmail account opened both the plain-text and multipart
Dovecot fixtures and saved the reference attachment with the expected
SHA-256. The reference IMAP trace showed requests for BODYSTRUCTURE,
BODY.PEEK[TEXT], and numbered MIME sections. Linkwise now serves these
requests. The regression verifies partial and whole-message retrieval,
including decoded attachment bytes.

The development-signed Rocky ARM64 RPM passed the package smoke test
and was installed with signature verification enabled. Its SHA-256 is
`46527eb135567332e6a93a5b8a482227a564ea36331144d5ac486d63175bbb54`.
Gmail submitted a fresh message over TLS using the Android-trusted
Linkwise CA. The test harness forwarded it through two virtual BW500
modems, verified both clean disconnects, and published the receiving
peer's copy to the test IMAP inbox. Gmail displayed the body and saved
the 1,400-byte attachment. The download matched the original byte for
byte, with SHA-256
`5af8f01f9fb8310f3815941a0629b40b45dd0b15f380640d02fc3f69073efd8b`.
All 17 Winlink Rust tests and the mail bridge regression passed.
Build, signature, modem and checksum logs are under
`mail-bench/gmail-mime-rpm`. The reference commands and downloaded
fixture are under `mail-bench/gmail-reference`.

## Apple Mail on the Mac mini with a private CA

Apple Mail on macOS 27.2.1 (26B131) tested the installed Rocky Linkwise
binary through SSH loopback forwards. The isolated IMAP and SMTP
listeners used implicit TLS on ports 11993 and 12565. A self-signed lab
root issued the localhost server certificate. Both listeners negotiated
TLS 1.3 when probed with that root. macOS rejected a mismatched hostname
even when supplied the root explicitly.

Apple Mail initially reported that the identity of localhost could not
be verified. After the operator approved login-keychain SSL trust
scoped to localhost, macOS verified the same certificate using the
default trust store. With TLS/SSL and password authentication set
explicitly in Mail, Connection Doctor reported successful connection
and login for both Linkwise IMAP and SMTP. The test inbox still showed
zero messages after refresh. Message delivery and attachment retrieval
in Apple Mail remain unverified. Evidence is under
`mail-bench/apple-mail-ca`.

The follow-up trace isolated the empty inbox to Apple's
`FETCH 1 (INTERNALDATE UID RFC822.SIZE FLAGS BODY.PEEK[HEADER])`.
Linkwise now returns the full RFC 5322 header section, including its
terminating empty line. Its exact command sequence passed the mail
bridge regression. The development-signed replacement RPM passed the
package smoke test and was installed with signature checking. Apple Mail
then listed the fixture, rendered its body, and showed the attachment.
The trace recorded successful header, BODYSTRUCTURE, and numbered
section fetches. Downloaded-file checksum and an Apple Mail-originated
BW500 round trip were not verified in this follow-up.
