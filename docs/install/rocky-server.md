# Linkwise server on Rocky Linux 9 ARM64

Use the Rocky Linux 9 ARM64 `1.rockymail` server RPM for production. The
recorded Rocky 9.8 ARM64 assessment used a separate receive-only package scope.
The server RPM includes the installed `linkwise` station manager,
`linkwise-modemd`, and the system OpenSSL SMTP and IMAP services.

## Build and install the server RPM

From the source checkout, select the exact Git commit for the candidate.
Run the build with an existing absolute evidence directory on an ARM64
container host or a host configured to run `linux/arm64` containers.

```sh
revision=$(git rev-parse HEAD)
evidence_dir=$(mktemp -d "${TMPDIR:-/tmp}/linkwise-rocky-rpm.XXXXXX")
tools/rocky9-vm/build-rpm.sh "$revision" "$evidence_dir" server
```

The wrapper uses Docker by default. Set `LINKWISE_CONTAINER_RUNTIME=podman`
on a host that uses Podman. The pinned Rocky container runs the build,
installs the RPM for a disposable smoke test, and exercises a managed BW500
exchange with the installed binaries and a checksum-verified waveform profile.
It records the archived commit in `build-revision.txt` and the test output in
`rocky-rpm-build.log`. The current server package name is
`linkwise-modem-0.1.0-1.rockymail.aarch64.rpm` under
`build-source/installer-artifacts/`. A native build on Rocky with
`openssl-devel` and the other RPM build prerequisites can use
`python3 tools/package-modem.py rpm --mail-tls`.

The disposable signature fixture writes the bench-signed copy and its public
key to `build-source/signing-bench/`. For deployment, use an approved release
signer, a distinct RPM version or release, and a checksum generated after
signing. Retain the signed RPM, checksum, source commit, and signer fingerprint
with the deployment record. A fresh install and a signed DNF upgrade are
separate qualification cases.

| Build | RPM release | Purpose |
| --- | --- | --- |
| `build-rpm.sh REVISION EVIDENCE_DIR server` | `1.rockymail` | Rocky server with system OpenSSL mail services |
| `build-rpm.sh REVISION EVIDENCE_DIR assessment` | `1` | Separate receive-only assessment scope |
| `package-modem.py rpm --full` | `3.rockyfull` | Feature-expanded Rocky Live USB package |

On the target host, import the approved release public key after verifying
its fingerprint through the site's trusted channel. From the directory
containing the release-signed RPM and its checksum, run the following with
`rpm` set to the selected artifact filename.

```sh
sha256sum -c "$rpm.sha256"
rpmkeys --checksig "$rpm"
sudo dnf --setopt=localpkg_gpgcheck=1 install "./$rpm"
rpm -q linkwise-modem
rpm -V linkwise-modem
```

## Verify the installed station manager

Run the installed station manager as an authorized local account through an
MCP stdio client. Its profile and private journal directory are separate from
`/etc/linkwise/station.json`, which configures native modem audio. Set
`LINKWISE_RADIO_BACKEND=simulated` and `LINKWISE_OLM_CONFIG` to the fixture's
station profile, then configure the MCP client to start `/usr/bin/linkwise`
with no arguments. In the `virtual_bw500` profile point `modem_path` to
`/usr/bin/linkwise-modemd` and `waveform_profile` to the pinned BW500 JSON.
Use a fresh local inventory file with one eligible peer callsign distinct from
the configured local callsign. [Profile and tool reference](../../rust/docs/mcp-usage.md)

With SELinux enforcing, refresh inventory, rank the peer, prepare, authorize,
and start one managed attempt. Expect the result to include clean virtual
cleanup and a SHA-256 for each verified P2P attachment and the waveform
profile. Retry the same start request and confirm it returns the original
attempt ID. Exercise abort, restart and journal recovery, verify key-up is
denied, and retain the package identity, SELinux process contexts, and
relevant audit records. The fixture uses ephemeral IPv4 loopback sockets and
requires no ALSA device. Physical PTT, RMS/CMS and production confinement
qualification remain separate gates. [Station integration status](../architecture/station-integration-status.md)

## Native audio modem

Configure the station callsign and the ALSA capture and playback devices
in `/etc/linkwise/station.json`. Set `bandwidth` to `500`, `2300`,
or `2750`. BW500 requires the matching `bw500_profile` JSON. Use an isolated
audio route for qualification. Physical PTT for the preview remains external,
and live use requires station-specific authorization, audio routing and
turnaround verification. Restore SELinux labels and validate the configuration.

```sh
sudo restorecon -R /etc/linkwise
sudo /usr/bin/linkwise-modemd --check-config /etc/linkwise/station.json
sudo systemctl enable --now linkwise-modem.service
```

## Mail access

Rocky must boot with kernel FIPS enabled and the system OpenSSL
FIPS policy active. The mail listeners dynamically use the installed
OpenSSL libraries and fail startup if the FIPS provider cannot load.
Confirm CMVP coverage for the exact packages and platform separately.

Create `/etc/linkwise-mail` as root with group `linkwise-mail` and mode
0750. Install the certificate chain as `server.pem` and the matching
key as `server.key`. Use root ownership and mode 0640 for the key, with
group `linkwise-mail`. The certificate must cover the DNS name clients
use. Place the following settings in root-owned 0600 environment files.
Choose strong passwords and supply the server's authorized callsign.

| File | Settings |
| --- | --- |
| `smtp.env` | `LINKWISE_CALLSIGN`, `LINKWISE_SMTP_USER`, `LINKWISE_SMTP_PASSWORD`, `LINKWISE_SMTP_BIND` |
| `imap.env` | `LINKWISE_IMAP_USER`, `LINKWISE_IMAP_PASSWORD`, `LINKWISE_IMAP_BIND` |

Bind both protocols to the approved client interface. The default is IPv4
loopback. Use only the client subnet in the firewall policy. Configure
Thunderbird for normal password authentication, SMTP STARTTLS on 1587,
and IMAP SSL/TLS on 1993. Install the appropriate CA trust on the client.

```sh
sudo restorecon -R /etc/linkwise-mail
sudo systemctl enable --now linkwise-mail-smtp.service linkwise-mail-imap.service
```

The services share `/var/lib/linkwise-mail`. Use `linkwise mail exchange`
or `linkwise mail listen` as the `linkwise-mail` account against that
mailbox and the local modem ports to forward queued messages and receive
mail from the peer. An operator must trigger the exchange or configure
a site-specific scheduler. IMAP exposes an Inbox and a pre-created
Sent folder. A client may save a Sent copy with IMAP APPEND.
Copies are stored separately under `imap-sent` and have no effect on the
radio transmit queue. The archive is append-only and capped at 64 MiB.
[Gmail for Android passed an attachment round trip over virtual BW500](../architecture/rocky-thunderbird-validation.md#gmail-attachment-round-trip-with-mime-section-retrieval).
That test used isolated listeners from the installed RPM with implicit TLS
for both SMTP and IMAP. The packaged SMTP unit uses STARTTLS on port 1587.
Gmail validation of that unit's STARTTLS configuration remains open.
