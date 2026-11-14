#!/usr/bin/env python3
"""Apply selected fixes from failed Rocky STIG rules and preserve the generated plan."""
import argparse
import re
import subprocess
from pathlib import Path
from xml.etree import ElementTree

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("results", type=Path)
parser.add_argument("--apply", action="store_true")
args = parser.parse_args()
tree = ElementTree.parse(args.results)
result = list(tree.findall("./{*}TestResult"))[-1]
generated = args.results.with_suffix(".generated.sh")
subprocess.run(
    ["oscap", "xccdf", "generate", "fix", "--result-id", result.attrib["id"],
     "--output", str(generated), str(args.results)],
    check=True,
)
# Leave authentication enrollment, network services, OS vendor settings and
# cryptographic policy to the environment-specific assessment.
eligible = re.compile(
    r"^(aide_check_audit_tools|aide_use_fips_hashes|aide_verify_acls|aide_verify_ext_attributes|"
    r"sshd_|disable_(host_auth|ctrlaltdel)|"
    r"audit_rules_(?!immutable|login_events)|audit_privileged_commands_|"
    r"file_permissions_audit_configuration_stig|sysctl_kernel_(?!kexec_load_disabled)|"
    r"sysctl_net_core_bpf_jit_harden|chronyd_(client_only|configure_local_socket|no_chronyc_network)|"
    r"accounts_umask_|grub2_page_poison_argument)"
)
text = generated.read_text()
prefix = "xccdf_org.ssgproject.content_rule_"
pattern = re.compile(
    rf"^# BEGIN fix .*? for '({prefix}[^']+)'\n"
    r".*?^\) # END fix for '\1'\s*$",
    re.MULTILINE | re.DOTALL,
)
blocks = [(rule, block) for match in pattern.finditer(text)
          for rule, block in [(match[1][len(prefix):], match[0])]
          if eligible.match(rule)]
if not blocks:
    raise SystemExit("No approved remediation blocks found")
selected = args.results.with_suffix(".selected.sh")
selected.write_text("#!/bin/bash\n" + "\n\n".join(block for _, block in blocks) + "\n")
subprocess.run(["bash", "-n", str(selected)], check=True)
for rule, _ in blocks:
    print(rule, flush=True)
if args.apply:
    subprocess.run(["bash", str(selected)], check=True)
    subprocess.run(["sshd", "-t"], check=True)
    subprocess.run(["systemctl", "reload", "sshd"], check=True)
