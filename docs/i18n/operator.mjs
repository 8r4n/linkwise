export const operatorSections = [
  ["start", ["linkwise-modemd --check-station station.json", "linkwise-modemd --serve-station station.json", "linkwise chat YOURCALL"]],
  ["chat", ["/listen on", "/connect PEERCALL", "/encrypt", "/proof message-proof.json"]],
  ["files", ["/sendfile PATH", "/accept 1 PATH", "/transfers", "/cancel 1"]],
  ["pki", ["linkwise pki check station-pki.json", "/cert offer --encrypt", "/cert inspect", "/cert accept SIGNING ENCRYPTION"]],
  ["qsl", ["linkwise qsl card station-pki.json BB2BB qsl.pdf \\\n  --utc 2026-11-15T19:30:00Z --freq-khz 7105 --mode BW500 --rst 59",
           "linkwise qsl receive station-pki.json AA1AA DIRECTORY SIGNING_SHA256 ENCRYPTION_SHA256"]],
  ["mail", ["linkwise mail import --pki station-pki.json --encrypt MAILBOX message.eml",
            "linkwise mail export --pki station-pki.json --encrypt MAILBOX MESSAGE_ID"]],
  ["receive", ["linkwise receive-pcm", "linkwise decode-wav capture.wav"]],
  ["security", []],
];

// A command keeps its literal reading order inside right-to-left prose.
const technical = /(VARA HF P2P → Settings → TNC Setup|P-256 \+ ML-KEM-768|\|[VEXF]|--[a-z][a-z-]+|\/[a-z][a-z-]+|127\.0\.0\.1)/g;

export function operatorRuns(text, direction) {
  return direction === "rtl"
    ? text.split(technical).map((text, index) => ({ text, isolate: index % 2 === 1 }))
    : [{ text, isolate: false }];
}
