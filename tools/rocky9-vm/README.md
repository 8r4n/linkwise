# Rocky Linux 9 ARM64 assessment VM

Use an Apple silicon Mac with UTM installed in `~/Applications`, Python 3,
Docker with ARM64 support, and at least 16 GiB of available host space. The
guest uses a 32 GiB sparse disk, 4 GiB of RAM, two vCPUs, and loopback TCP
ports 2224 for SSH and 2234 for serial. The VM bundle and evidence directory
must be new for creation. UTM uses its local configuration approval and
registration UI if prompted.

Run from the repository root. The installer shuts down when it finishes, so
run the first-boot phase from a second terminal or after creation returns.

```sh
tools/rocky9-vm/create.sh
tools/rocky9-vm/after-install.sh
evidence="$HOME/Virtual Machines/Linkwise Rocky9 Evidence"
tools/rocky9-vm/fetch-stig.sh "$evidence"
tools/rocky9-vm/build-rpm.sh a2d01b73a88c0968995b767001684ad0f533fddc "$evidence" assessment
tools/rocky9-vm/assess.sh
```

Creation verifies the pinned boot ISO and the PXE kernel and initramfs. The
Kickstart sets up LUKS2 and encrypted data volumes. The guest repositories
are rolling Rocky 9 mirrors, so record the resulting package inventory with
each run. The build container pins the Rocky image and Rust toolchain. RPM
signing uses a disposable bench key whose public portion supplies the test
trust anchor. Use a release signer for distributable packages.

The evidence directory is private and contains the disk passphrase, SSH
material, administrator credential and serial logs. The private Kickstart
and its OEMDRV image also contain credentials. Detach the image after
installation. Keep generated media and credentials outside Git. The serial
scripts use the per-VM passphrase from the evidence directory. For manual
unlock, read the passphrase from that directory on the host.

The FIPS boot UUID is set during installation. On a prior guest missing this
setting, pass its /boot filesystem UUID as the second argument to
`recovery-grub.py` during GRUB startup. After boot use
`grubby --update-kernel=ALL --args="boot=UUID=<boot-filesystem-uuid> audit_backlog_limit=8192"`
to persist the repair. Run `reboot.sh` to verify a cold boot and SSH access.

`assess.sh` captures both benchmarks, deploys Linkwise, applies scoped
hardening, preserves the original and selected OpenSCAP-generated fix scripts,
and verifies service operation on the hardened guest. Use
`assess.sh post-reboot` on an already staged guest to collect a fresh scan,
package inventory, service verification and ALSA receive test. Review the
findings in the [assessment](../../docs/architecture/niap-rocky9-assessment.md).

The assessment uses the full, unmodified Rocky Security Guide STIG profile.
The original DISA RHEL scan is retained as reference evidence. Record
Rocky-specific substitutions and manual checks in the
[disposition register](../../docs/architecture/niap-rocky-stig-findings.md).

For the two-way mail deployment, see the [Rocky server guide](../../docs/install/rocky-server.md).
