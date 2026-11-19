# Linkwise website demonstrations

These recordings use isolated fixtures and scripted test payloads.
The WebP posters are frames extracted from their paired videos. The Winlink
poster comes from the [PRBS9 fixture](../modem/winlink-prbs9/winlink-prbs9.mp4).

| Recording | Evidence and reproduction |
| --- | --- |
| [Rocky Live USB chat and mail](rocky-live-usb.mp4) | Two ARM64 UTM guests, separate virtual USB disks, virtual BW500 chat, and B2F attachment hashes. See the [live media bench](../../../tools/rocky-live-usb/README.md). |
| [Rocky Mutt mailbox](rocky-mutt.mp4) | B2F mail received on the virtual bench and opened in Mutt. See the [live media bench](../../../tools/rocky-live-usb/README.md). |
| [Rocky PKI mail](rocky-pki-mail.mp4) | Signed and encrypted B2F mail verified on the virtual bench. See the [PKI runbook](../../../rust/docs/pki.md). |
| [Interactive Mutt PKI](rocky-interactive-mutt.mp4) | Mutt Send queues a protected message. The peer verifies the attachment, and an unknown recipient leaves the draft intact. See the [Mutt runbook](../../../tools/rocky-live-usb/README.md#read-and-compose-email-with-mutt). |
| [Modem robustness briefing](robustness-briefing.mp4) | The research chronology and planned qualification from [test plan revision 0.11](../../testing/kiwisdr-modem-test-plan.pdf). Read the [narration transcript](robustness-briefing-transcript.md). |
