Name: linkwise-modem
Version: %{linkwise_version}
Release: 1
Summary: Linkwise modem preview
License: LicenseRef-Unspecified
BuildRequires: systemd-rpm-macros, selinux-policy-devel, make
Requires(post): policycoreutils
Requires(postun): policycoreutils
%{?systemd_requires}
URL: https://github.com/8r4n/linkwise

%description
Station diagnostics and local modem simulations for amateur radio.

%build
make -C "%{_topdir}/selinux" -f /usr/share/selinux/devel/Makefile linkwise_modem.pp

%install
mkdir -p "%{buildroot}%{_prefix}"
cp -a "%{_topdir}/payload/usr/bin" "%{buildroot}%{_prefix}/"
install -Dm644 "%{_topdir}/payload/usr/lib/systemd/system/linkwise-modem.service" "%{buildroot}%{_unitdir}/linkwise-modem.service"
install -Dm644 "%{_topdir}/payload/usr/lib/systemd/user/linkwise-modem.service" "%{buildroot}%{_userunitdir}/linkwise-modem.service"
mkdir -p "%{buildroot}%{_docdir}"
cp -a "%{_topdir}/payload/usr/share/doc/linkwise-modem" "%{buildroot}%{_docdir}/"

install -Dm644 "%{_topdir}/selinux/linkwise_modem.pp" "%{buildroot}%{_datadir}/selinux/packages/linkwise_modem.pp"
install -Dm644 "%{_topdir}/selinux/linkwise_modem_ports.cil" "%{buildroot}%{_datadir}/selinux/packages/linkwise_modem_ports.cil"
install -Dm644 "%{_topdir}/80-linkwise-modem.preset" "%{buildroot}%{_presetdir}/80-linkwise-modem.preset"
install -Dm644 "%{_topdir}/80-linkwise-modem.preset" "%{buildroot}%{_userpresetdir}/80-linkwise-modem.preset"

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
%systemd_preun linkwise-modem.service
%systemd_user_preun linkwise-modem.service

%postun
%systemd_postun_with_restart linkwise-modem.service
%systemd_user_postun_with_restart linkwise-modem.service

if [ "$1" -eq 0 ] && selinuxenabled; then
    semodule -X 200 -r linkwise_modem_ports -r linkwise_modem || exit 1
    if [ -d /etc/linkwise ]; then
        restorecon -R /etc/linkwise || exit 1
    fi
fi

%files
%{_bindir}/linkwise
%{_bindir}/linkwise-modemd
%doc %{_docdir}/linkwise-modem/
%{_unitdir}/linkwise-modem.service
%{_userunitdir}/linkwise-modem.service
%{_presetdir}/80-linkwise-modem.preset
%{_userpresetdir}/80-linkwise-modem.preset
%{_datadir}/selinux/packages/linkwise_modem.pp
%{_datadir}/selinux/packages/linkwise_modem_ports.cil
