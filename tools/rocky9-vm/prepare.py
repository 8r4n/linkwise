from pathlib import Path
import os, plistlib, subprocess
p=Path.home()/'Virtual Machines'/'Linkwise Rocky9 Evidence'
v=p.parent/'Linkwise Rocky 9 ARM64.utm'
secret=(p/'disk-unlock.txt').read_text().strip()
password=(p/'admin-password.txt').read_text().strip()
key=(p/'admin-rsa.pub').read_text().strip()
ks=f"""text
lang en_US.UTF-8
keyboard us
timezone UTC --utc
network --bootproto=dhcp --device=link --activate --hostname=linkwise-rocky9
url --url=https://download.rockylinux.org/pub/rocky/9/BaseOS/aarch64/os/
repo --name=AppStream --baseurl=https://download.rockylinux.org/pub/rocky/9/AppStream/aarch64/os/
rootpw --lock
user --name=lwadmin --groups=wheel --plaintext --password={password}
sshkey --username=lwadmin "{key}"
firewall --enabled --service=ssh
selinux --enforcing
services --enabled=sshd,auditd,firewalld
firstboot --disable
bootloader --append="fips=1 audit=1 console=tty0 console=ttyAMA0"
ignoredisk --only-use=vda
zerombr
clearpart --all --initlabel --drives=vda
part /boot/efi --fstype=efi --size=512 --ondisk=vda
part /boot --fstype=xfs --size=1024 --ondisk=vda
part pv.01 --size=1 --grow --ondisk=vda --encrypted --luks-version=luks2 --pbkdf=pbkdf2 --passphrase={secret}
volgroup system pv.01
logvol / --vgname=system --name=root --fstype=xfs --size=6500 --grow
logvol /home --vgname=system --name=home --fstype=xfs --size=1024
logvol /tmp --vgname=system --name=tmp --fstype=xfs --size=1024
logvol /var --vgname=system --name=var --fstype=xfs --size=4096
logvol /var/tmp --vgname=system --name=vartmp --fstype=xfs --size=1024
logvol /var/log --vgname=system --name=varlog --fstype=xfs --size=1024
logvol /var/log/audit --vgname=system --name=audit --fstype=xfs --size=1024
logvol swap --vgname=system --name=swap --fstype=swap --size=512
poweroff
%packages
@^minimal-environment
crypto-policies-scripts
openscap-scanner
scap-security-guide
policycoreutils-python-utils
audit
alsa-utils
%end
%post --erroronfail
fips-mode-setup --enable --no-bootcfg
boot_uuid=$(blkid -s UUID -o value /dev/vda2)
test -n "$boot_uuid"
grubby --update-kernel=ALL --args="boot=UUID=$boot_uuid audit_backlog_limit=8192"
install -d -m 0755 /etc/ssh/sshd_config.d
printf '%s\\n' 'PermitRootLogin no' 'PasswordAuthentication no' 'KbdInteractiveAuthentication no' > /etc/ssh/sshd_config.d/01-linkwise.conf
rm -f /root/anaconda-ks.cfg /root/original-ks.cfg
%end
"""
os.umask(0o077)
(p/'oemdrv').mkdir(mode=0o700, exist_ok=True)
(p/'oemdrv'/'ks.cfg').write_text(ks)
subprocess.run(["hdiutil", "makehybrid", "-iso", "-joliet", "-default-volume-name", "OEMDRV",
                "-o", str(p/"OEMDRV.iso"), str(p/"oemdrv")], check=True)
cfg=plistlib.loads((v/'config.plist').read_bytes())
import uuid
for name in ('vmlinuz', 'initrd.img', 'OEMDRV.iso', 'Rocky-9.8-aarch64-boot.iso'):
    source=p/name
    target=v/'Data'/name
    if not target.exists():
        source.rename(target)
cfg['Drive']=cfg['Drive'][:1]
for name in ('OEMDRV.iso','Rocky-9.8-aarch64-boot.iso'):
    cfg['Drive'].append(dict(Identifier=str(uuid.uuid4()).upper(), ImageName=name,
                             ImageType='Disk', Interface='VirtIO', InterfaceVersion=1, ReadOnly=True))
cfg['QEMU']['AdditionalArguments']=[
    '-kernel', f'"{v/"Data/vmlinuz"}"', '-initrd', f'"{v/"Data/initrd.img"}"',
    '-append', '"earlycon=pl011,mmio32,0x09000000 console=tty0 console=ttyAMA0 inst.text inst.ks=hd:LABEL=OEMDRV:/ks.cfg inst.stage2=hd:/dev/vdc ip=dhcp"']
(v/'config.plist').write_bytes(plistlib.dumps(cfg))
