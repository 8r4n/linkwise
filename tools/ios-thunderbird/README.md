# Thunderbird for iOS simulator validation

The benchmark uses Thunderbird's unmodified SMTP, IMAP, and MIME libraries
from revision `a8de4c481a1ce692f3961960d4019251c4f57444`. A Swift test supplies
the attachment directly because the app's Files picker and Send integration
are unfinished at that revision. This checks direct authenticated mail
submission and retrieval with two Linkwise modems on clean virtual BW500.

## Build

From the Linkwise checkout:

```sh
cargo build --locked --manifest-path rust/Cargo.toml --features mail-tls --bins
python3 tools/fetch-modem-profile.py
bench_dir=$(mktemp -d)
git clone https://github.com/thunderbird/thunderbird-ios.git "$bench_dir/thunderbird"
git -C "$bench_dir/thunderbird" checkout a8de4c481a1ce692f3961960d4019251c4f57444
cp tools/ios-thunderbird/LinkwiseBenchTests.swift \
  "$bench_dir/thunderbird/Core/Tests/SMTPTests/"
```

In Thunderbird's `Core/Package.swift`, add `"IMAP", "MIME"` to the
`SMTPTests` target dependencies. Boot an iPhone simulator and set `simulator`
to its UDID. The reference run used Xcode 27.2 and an iPhone 17 simulator
running iOS 27.2.

## Configure trust

Use a disposable test simulator. Generate a short-lived bench CA and a server
certificate with DNS SAN `localhost`. Import the CA into that simulator only.
The example uses OpenSSL with `req -addext` support.

```sh
bash tools/ios-thunderbird/make-certificates.sh "$bench_dir/tls"
xcrun simctl keychain "$simulator" add-root-cert "$bench_dir/tls/ca.pem"
```

## Run

Build the test from the Linkwise checkout, then start the Linkwise bench in
one terminal. Wait for `BENCH READY` before running the simulator test from a
second terminal with the same `bench_dir` and `simulator` values.

```sh
xcodebuild -workspace "$bench_dir/thunderbird/Thunderbird.xcworkspace" \
  -scheme ThunderbirdTests -destination "platform=iOS Simulator,id=$simulator" \
  -derivedDataPath "$bench_dir/build" CODE_SIGNING_ALLOWED=NO \
  '-only-testing:SMTPTests/linkwiseAttachmentRoundTrip()' build-for-testing

python3 tools/ios-thunderbird/bench.py "$bench_dir/mailboxes" "$bench_dir/tls"
```

```sh
xcodebuild test-without-building \
  -xctestrun "$(find "$bench_dir/build/Build/Products" -name '*.xctestrun' -print -quit)" \
  -destination "platform=iOS Simulator,id=$simulator" -parallel-testing-enabled NO \
  '-only-testing:SMTPTests/linkwiseAttachmentRoundTrip()' \
  -resultBundlePath "$bench_dir/linkwise.xcresult"
xcrun xcresulttool get test-results summary --path "$bench_dir/linkwise.xcresult"
```

Require one passing Swift test, the same SHA-256 in the modem and Swift
output, a confirmed sender queue marker, one recipient inbox copy, and
successful disconnects on both modems. The reference attachment hash is
`5af8f01f9fb8310f3815941a0629b40b45dd0b15f380640d02fc3f69073efd8b`.

The fixture endpoints are IPv4 loopback SMTP STARTTLS on 12525 and IMAP
implicit TLS on 11143. Fixture credentials are `bench` / `fixture`.
The Python preflight checks rejection of plaintext authentication, bad
passwords, mismatched senders, interrupted SMTP DATA, untrusted certificates,
and hostname mismatches.

Physical iPhone, stock attachment UI, and impaired-channel tests are
separate qualification steps. The loopback fixture does not provide access
from a physical iPhone.

To exercise the combined HF profiles, build the channel image first.

```sh
docker build -t linkwise-hf-bench containers/hf-bench
python3 tools/ios-thunderbird/bench.py "$bench_dir/hf-32-17" "$bench_dir/tls" \
  --hf-worker tools/ios-thunderbird/hf-worker.py --snr 32 --seed 17
```

Use a fresh mailbox directory and result bundle for each run. The 32 dB
profile uses a 1 ms echo at -9 dB and 0.5 Hz Doppler spread. The 22 dB
profile uses a 2 ms echo at -4 dB and 1 Hz spread. Both use Rician K = 10.

## Reference results

The clean simulator trial passed with direct SMTP STARTTLS and implicit
IMAP TLS. The modem log and Swift test reported the same 1,400-byte
attachment hash. Both endpoints disconnected cleanly and the sender
marked its queue entry confirmed.

| Channel | Seed | Attachment and modem result |
| --- | --- | --- |
| 32 dB | 17 | The simulator retrieved the exact attachment. The modem reported unconfirmed bytes and failed clean teardown. |
| 32 dB | 23 | DATA acknowledgment timed out. The message remained queued. |
| 22 dB | 18 | The modem disconnected before B2F exchange. The message remained queued. |
| 22 dB | 24 | The modem disconnected before B2F exchange. The message remained queued. |

A clean-channel retry of the queued message from the 32 dB seed 23
trial produced exactly one recipient copy, a matching attachment hash,
a confirmed sender marker, and clean disconnects. A further empty
same-modem exchange timed out in a separate attempt.
