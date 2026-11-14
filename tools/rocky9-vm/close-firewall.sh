#!/bin/bash
set -euo pipefail
zone=$(firewall-cmd --get-default-zone)
# Retain the existing host-only SSH management path. The guest uses IPv4 DHCP.
firewall-cmd --permanent --zone="$zone" --query-service=ssh
for service in cockpit dhcpv6-client; do
    if firewall-cmd --permanent --zone="$zone" --query-service="$service"; then
        firewall-cmd --permanent --zone="$zone" --remove-service="$service"
    fi
done
if firewall-cmd --permanent --zone="$zone" --query-forward; then
    firewall-cmd --permanent --zone="$zone" --remove-forward
fi
firewall-cmd --permanent --zone="$zone" --set-target=DROP
firewall-cmd --reload
firewall-cmd --zone="$zone" --list-all
