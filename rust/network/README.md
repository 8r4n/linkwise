# Linkwise adaptive network bench

This standalone Rust crate runs a deterministic contact simulator and independent store-and-forward stations. It uses a separate database per station. The station authenticates commissioned neighbors with per-link keys, learns expiring path vectors, and opportunistically calls neighbors with a route to queued traffic. It reserves alternate call opportunities for discovery. Data and end-to-end application receipts travel as separate BPv7 bundles. A receiver commits the bundle before acknowledging a hop. Receipts rely on trusted relays and do not provide end-to-end cryptographic origin authentication.

## Reproduce

Run these commands in this directory.

```sh
cargo build --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
target/debug/linkwise-network simulate six
target/debug/linkwise-network simulate global-100 17 1
target/debug/linkwise-network scenario global-30 > scenario.json
target/debug/linkwise-network simulate scenario.json 17 30
python3 tests/autonomous.py target/debug/linkwise-network
python3 tests/autonomous.py target/debug/linkwise-network --live-queue
```

The Python fixture shows complete station profiles and how to commission a topology, submit binary data, run the daemons, read the result, and check the destination receipt. The manual `send` and `listen` commands assume a trusted local byte stream. The modem fixture accepts the network binary, an existing `linkwise-modemd` binary, and a BW500 level-4 profile as positional arguments.

For the independent codec check, build tests/interop with its own lockfile, export a queued A-to-B bundle with `export DB A ID`, then run the interop binary with that file and an output filename. Import its output with `import DB A FILE` and use `read DB A ID` to retrieve the four binary bytes 00 ff 0a 0d.

## Submit and read while a station is running

On Unix, add an optional `control_socket` path to the station profile. Put the socket in a directory accessible only to the station owner, such as one created with `mkdir -m 700 /path/to/private`. The same OS account can invoke these commands while `serve` owns the database.

```sh
linkwise-network queue /path/to/private/A.sock submit request-1 F message.bin 300
linkwise-network queue /path/to/private/A.sock list
linkwise-network queue /path/to/private/F.sock read BUNDLE_ID > received.bin
```

The submission returns a bundle ID once the queue entry commits. Reuse the same request ID, destination, bytes, and lifetime if the response was lost. Reusing a request ID for a different submission returns an error. The request ID is retained until the bundle expiry plus the seven-day receipt retention window. `list` shows queue state and whether a destination receipt reached the source. `read` returns the destination's committed payload bytes. The original database CLI commands remain available when the daemon is stopped.

`serve` removes its socket on normal exit. After a forced stop, remove the stale socket file once the previous daemon has exited. The socket protocol uses one length-prefixed JSON command and reply per connection. The `queue` CLI handles binary input and output.

## Observed behavior

| Experiment | Delivered before deadline or verified outcome |
| --- | --- |
| Six-node scripted contact plan | Fixed forwarding expired. Time-aware forwarding waited and delivered over slots 5, 7 and 10. |
| 30 synthetic global nodes, 30 paired seeds | All four policies delivered 900 of 900 offers. |
| 100 nodes, seed 17 | Fixed 2/100, contact adaptation alone 15/100, route adaptation alone 76/100, both 78/100. |
| 1,000 nodes, seed 17 | Fixed 0/1,000, contact adaptation alone 0/1,000, route adaptation alone 2/1,000, both 2/1,000. |
| Independent station processes | With B offline, A delivered 4,352 bytes via D and E to F and recorded the destination receipt. |
| Two virtual BW500 modem engines | A 32-byte binary application payload survived a complete transfer and readback using unpaced virtual audio. |

The simulator uses synthetic regional time windows, fixed contact capacities, and slot-level interference. Its time-aware policy sees the commissioned schedule. The daemon sees neighbor advertisements and uses a rotating discovery schedule. Simulator counts therefore measure the modeled policies. The process fixture tests actual daemon behavior. The 1,000-node result exposes overload in this scenario. Field performance, energy consumption, physical PTT and propagation calibration remain unverified.

The many-station encrypted-chat regression launches eight real chat clients and eight virtual BW500 modem processes in four simultaneous pairs. Run
`python3 tests/encrypted_chat.py CLIENT MODEM BW500_PROFILE` with a PKI-enabled secure-transfer client. Each pair uses fresh credentials, verifies both peers and exact text in both directions, verifies a saved message proof, rejects a modified signature, and exits. These are direct virtual audio links. Relay routing is exercised separately by the network bench.

In three further 1,000-node simulations (seeds 40 through 42), combined adaptation delivered 1, 1, and 0 messages respectively out of 1,000 per seed. Route adaptation alone matched those counts. Both fixed-route policies delivered zero across all three runs.

Add `--network target/debug/linkwise-network` to the encrypted-chat command to require three durable BP transfers of every outbound DATA chunk before it enters the virtual modem. The fixture uses independent queues in each direction, sends A–D–E–F and F–E–D–A, and compares the recovered bytes before releasing them to the modem. Four simultaneous pairs passed with 36 bundles and 108 successful hop transfers. Chat commands and modem control stay local. This fixture selects the three hops explicitly. The autonomous station fixture separately verifies route discovery and return receipts with an offline peer.
