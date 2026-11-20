import { repository } from "./releases.mjs";

export const supportLinks = {
  releases: `${repository}/releases`,
  reportBug: `${repository}/issues/new?template=bug-report.yml`,
  knownIssues: `${repository}/issues?q=is%3Aissue+is%3Aopen`,
};

export const audioSamples = [
  ["bw500-level1-clean.wav", "BW500 level 1 · clean", "3 frames verified"],
  ["bw500-level2-clean.wav", "BW500 level 2 · clean", "1 frame verified"],
  ["bw500-level3-clean.wav", "BW500 level 3 · clean", "1 frame verified"],
  ["bw500-level3-snr24db.wav", "BW500 level 3 · 24 dB added-noise SNR", "1 frame verified"],
];

export const projectMedia = [
  ["docs/media/modem/modem-demo.png", "modem-demo.png"],
  ["docs/media/modem/winlink-prbs9/winlink-prbs9.mp4", "winlink-prbs9.mp4"],
  ["docs/media/modem/modem-streaming.gif", "media/modem-streaming.gif"],
  ...audioSamples.map(([name]) => [`docs/media/modem/${name}`, `media/${name}`]),
  ...["hf-routing-results.mp4", "hf-routing-results.webp", "hf-routing-results.vtt", "three-hf-links.mp4", "three-hf-links.webp", "rocky-live-usb.mp4", "rocky-mutt.mp4", "rocky-pki-mail.mp4", "rocky-interactive-mutt.mp4",
    "robustness-briefing.mp4", "robustness-briefing-transcript.md", "winlink-prbs9.webp", "rocky-live-usb.webp",
    "rocky-interactive-mutt.webp", "robustness-briefing.webp"].map(name => [`docs/media/demos/${name}`, `media/${name}`]),
  ["docs/testing/figures/kiwisdr-beacon-preview-1.png", "media/beacon-range-rings.png"],
  ["docs/testing/figures/kiwisdr-beacon-preview-2.png", "media/beacon-frame-fields.png"],
  ["lab-notes/figures/kiwisdr-beacon-map-dsp.pdf", "media/kiwisdr-beacon-map-dsp.pdf"],
  ["lab-notes/lab-notes.pdf", "media/lab-notes.pdf"],
  ["docs/testing/kiwisdr-modem-test-plan.pdf", "media/modem-robustness-test-plan.pdf"],
];

export const capabilities = [
  ["HF modem", "Audio-enabled source build. BW500 two-way operation uses a measurement profile.",
    "Mixed stock VARA HF 4.9.1 DATA exchanges passed on the isolated bench at BW500, BW2300, and BW2750. Physical RF qualification remains open.",
    "rust/docs/modem.md"],
  ["VarAC terminal chat", "BW500 station daemon and the Linkwise terminal client.",
    "Direct chat was exercised with mixed peers. Passive KiwiSDR reception has produced exact beacon payloads. Conversation reconstruction requires session reassembly.",
    "docs/testing/varac-chat.md"],
  ["File transfer", "The secure-transfer build enables gzip and SHA-256 checks. Encrypted file transfers use a dedicated mutually authenticated TLS 1.3 session.",
    "Linkwise file transfers were exercised on the bench. The client declines stock VarAC MD5 file offers.",
    "rust/docs/secure-transfer.md"],
  ["Station identity, QSL and protected chat", "Shared PKI in a secure-transfer build. Optional hybrid P-256 and ML-KEM-768 chat also requires the pqc-chat build and a qualifying OpenSSL FIPS provider configuration.",
    "Linkwise verifies station certificates and message signatures. Exchange public certificates on printable QSL cards or import them from photos.",
    "rust/docs/pki.md"],
  ["Winlink and direct mail", "A Linkwise modem endpoint for Winlink Express and Pat. The CLI also offers Open B2F forwarding between Linkwise stations.",
    "Mixed P2P attachment transfers passed on isolated audio paths. Live gateway and physical RF qualification remain open.",
    "docs/testing/vara-interoperability.md"],
  ["Desktop and mobile mail clients", "SMTP and IMAP access requires a mail-tls build for TLS. Protected radio mail uses a shared PKI profile.",
    "Evidence includes macOS client login and display, Android emulator attachment exchange, and iOS simulator protocol-library tests. Client and channel coverage varies.",
    "docs/architecture/rocky-thunderbird-validation.md"],
  ["Rocky Linux 9 Live USB and Mutt", "Experimental ARM64 live media built with the Rocky tooling.",
    "Two UTM guests booted separate virtual USB disks and exchanged chat and B2F mail over virtual BW500. Interactive Mutt verified signed and encrypted delivery. Physical USB and x64 qualification remain open.",
    "tools/rocky-live-usb/README.md"],
  ["Receive-only monitoring", "A station or recording supplying 48 kHz mono signed 16-bit PCM to receive-pcm.",
    "KiwiSDR replay recovered CRC-valid VarAC beacons. Passive monitoring cannot establish a field recovery rate without a known transmission count.",
    "rust/docs/kiwisdr-test-plan.md"],
];

export const acquisitionEvidence = `${repository}/blob/b16215d8718a9ba508850657d298094f9cfb08c6/lab-notes/lab-notes.tex#L3298-L3377`;
export const laterCWResults = `${repository}/blob/b16215d8718a9ba508850657d298094f9cfb08c6/lab-notes/lab-notes.tex#L4219-L4240`;

export const matchedReplay = [
  { snr: -8, baseline: 5, updated: 10, stock: 10 },
  { snr: -4, baseline: 10, updated: 10, stock: 10 },
  { snr: 0, baseline: 10, updated: 20, stock: 15 },
  { snr: 4, baseline: 11, updated: 20, stock: 20 },
];
export const trialsPerLevel = 20;
