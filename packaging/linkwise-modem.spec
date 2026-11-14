Name: linkwise-modem
Version: %{linkwise_version}
Release: %{?linkwise_release}%{!?linkwise_release:1}
Summary: Linkwise modem preview
License: Apache-2.0
BuildRequires: systemd-rpm-macros, selinux-policy-devel, make
%if "%{?linkwise_release}" == "1.rockymail"
Requires: openssl-fips-provider
Requires(pre): shadow-utils
%endif
Requires(post): policycoreutils
Requires(postun): policycoreutils
%{?systemd_requires}
URL: https://github.com/8r4n/linkwise

%description
Station diagnostics and local modem simulations for amateur radio.

%build
make -C "%{_topdir}/selinux" -f /usr/share/selinux/devel/Makefile linkwise_modem.pp

%install
%if "%{?linkwise_release}" == "1.rockymail"
for service in smtp imap; do
    install -Dm644 "%{_topdir}/payload/usr/lib/systemd/system/linkwise-mail-$service.service" "%{buildroot}%{_unitdir}/linkwise-mail-$service.service"
done
%endif
mkdir -p "%{buildroot}%{_prefix}"
cp -a "%{_topdir}/payload/usr/bin" "%{buildroot}%{_prefix}/"
install -Dm644 "%{_topdir}/payload/usr/lib/systemd/system/linkwise-modem.service" "%{buildroot}%{_unitdir}/linkwise-modem.service"
install -Dm644 "%{_topdir}/payload/usr/lib/systemd/user/linkwise-modem.service" "%{buildroot}%{_userunitdir}/linkwise-modem.service"
mkdir -p "%{buildroot}%{_docdir}"
cp -a "%{_topdir}/payload/usr/share/doc/linkwise-modem" "%{buildroot}%{_docdir}/"
install -Dm644 "%{_topdir}/payload/usr/share/man/man1/linkwise.1" "%{buildroot}%{_mandir}/man1/linkwise.1"
install -Dm644 "%{_topdir}/payload/usr/share/man/man8/linkwise-modemd.8" "%{buildroot}%{_mandir}/man8/linkwise-modemd.8"
install -Dm644 "%{_topdir}/payload/usr/share/man/man5/linkwise-station.5" "%{buildroot}%{_mandir}/man5/linkwise-station.5"

install -Dm644 "%{_topdir}/selinux/linkwise_modem.pp" "%{buildroot}%{_datadir}/selinux/packages/linkwise_modem.pp"
install -Dm644 "%{_topdir}/selinux/linkwise_modem_ports.cil" "%{buildroot}%{_datadir}/selinux/packages/linkwise_modem_ports.cil"
install -Dm644 "%{_topdir}/80-linkwise-modem.preset" "%{buildroot}%{_presetdir}/80-linkwise-modem.preset"
install -Dm644 "%{_topdir}/80-linkwise-modem.preset" "%{buildroot}%{_userpresetdir}/80-linkwise-modem.preset"

%if "%{?linkwise_release}" == "1.rockymail"
%pre
getent group linkwise-mail >/dev/null || groupadd -r linkwise-mail || exit 1
getent passwd linkwise-mail >/dev/null || useradd -r -g linkwise-mail -d /var/lib/linkwise-mail -s /sbin/nologin linkwise-mail || exit 1
%endif

%post
if selinuxenabled; then
    semodule -X 200 -i %{_datadir}/selinux/packages/linkwise_modem.pp -i %{_datadir}/selinux/packages/linkwise_modem_ports.cil || exit 1
    restorecon %{_bindir}/linkwise-modemd || exit 1
    if [ -d /etc/linkwise ]; then
        restorecon -R /etc/linkwise || exit 1
    fi
fi
%systemd_post linkwise-modem.service
%systemd_user_post linkwise-modem.service

%preun
%if "%{?linkwise_release}" == "1.rockymail"
%systemd_preun linkwise-mail-smtp.service linkwise-mail-imap.service
%endif
%systemd_preun linkwise-modem.service
%systemd_user_preun linkwise-modem.service

%postun
%if "%{?linkwise_release}" == "1.rockymail"
%systemd_postun_with_restart linkwise-mail-smtp.service linkwise-mail-imap.service
%endif
%systemd_postun_with_restart linkwise-modem.service
%systemd_user_postun_with_restart linkwise-modem.service

if [ "$1" -eq 0 ] && selinuxenabled; then
    semodule -X 200 -r linkwise_modem_ports -r linkwise_modem || exit 1
    if [ -d /etc/linkwise ]; then
        restorecon -R /etc/linkwise || exit 1
    fi
fi

%files
%if "%{?linkwise_release}" == "1.rockymail"
%{_unitdir}/linkwise-mail-smtp.service
%{_unitdir}/linkwise-mail-imap.service
%endif
%{_bindir}/linkwise
%{_bindir}/linkwise-modemd
%license %{_docdir}/linkwise-modem/LICENSE
%license %{_docdir}/linkwise-modem/THIRD-PARTY-NOTICES.txt
%doc %{_docdir}/linkwise-modem/*.md
%{_mandir}/man1/linkwise.1*
%{_mandir}/man8/linkwise-modemd.8*
%{_mandir}/man5/linkwise-station.5*
%{_unitdir}/linkwise-modem.service
%{_userunitdir}/linkwise-modem.service
%{_presetdir}/80-linkwise-modem.preset
%{_userpresetdir}/80-linkwise-modem.preset
%{_datadir}/selinux/packages/linkwise_modem.pp
%{_datadir}/selinux/packages/linkwise_modem_ports.cil
