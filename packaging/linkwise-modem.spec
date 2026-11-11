Name: linkwise-modem
Version: %{linkwise_version}
Release: 1
Summary: Linkwise modem preview
License: LicenseRef-Unspecified
URL: https://github.com/8r4n/linkwise

%description
Station diagnostics and local modem simulations for amateur radio.

%install
mkdir -p "%{buildroot}"
cp -a "%{_topdir}/payload/." "%{buildroot}/"

%files
%{_bindir}/linkwise
%{_bindir}/linkwise-modemd
%doc %{_docdir}/linkwise-modem/
