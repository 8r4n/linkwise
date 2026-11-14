# Linkwise on Rocky Linux 9

Build the server RPM with `tools/rocky9-vm/build-rpm.sh REVISION EVIDENCE_DIR`
or `python3 tools/package-modem.py rpm --mail-tls` on Rocky with
`openssl-devel`. Install the signed RPM using DNF with local signature
verification enabled. The assessment-only profile is a separate build.

## Two-way modem

Configure the station callsign and the ALSA capture and playback devices
in `/etc/linkwise/station.json`. Set `bandwidth` to `500`, `2300`,
or `2750`. BW500 requires the matching `bw500_profile` JSON. For
production radio operation, verify audio routing, transmit authorization,
PTT or VOX, and turnaround before connecting the radio. Restore SELinux
labels and validate the configuration.

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
