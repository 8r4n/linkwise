set -euo pipefail
candidate=$(find /src/installer-artifacts -maxdepth 1 -name '*.rpm' -print -quit)
test -n "$candidate"
evidence=/src/signing-bench
mkdir "$evidence"
work=$(mktemp -d)
export GNUPGHOME="$work/keys"
mkdir -m 700 "$GNUPGHOME"
trap 'gpgconf --kill gpg-agent; rm -rf "$work"' EXIT
gpg --batch --passphrase '' --quick-generate-key 'Linkwise NIAP bench only <bench@localhost>' rsa2048 sign 1d
fingerprint=$(gpg --batch --with-colons --list-keys | awk -F: '$1=="fpr"{print $10; exit}')
gpg --batch --armor --export "$fingerprint" >"$evidence/bench-public.asc"
signed="$evidence/${candidate##*/}"
cp "$candidate" "$signed"
rpmsign --define "_openpgp_sign_id $fingerprint" --define "_gpg_name $fingerprint" --addsign "$signed"
mkdir "$work/trusted" "$work/empty"
rpmkeys --dbpath "$work/trusted" --import "$evidence/bench-public.asc"
rpmkeys --dbpath "$work/trusted" --checksig "$signed"
if rpmkeys --dbpath "$work/empty" --checksig "$signed"; then
    echo "Unexpected acceptance of unknown signing key"
    exit 1
fi
python3 - "$signed" "$work/tampered.rpm" <<'PY'
from pathlib import Path
import sys
data = bytearray(Path(sys.argv[1]).read_bytes())
data[-10] ^= 1
Path(sys.argv[2]).write_bytes(data)
PY
if rpmkeys --dbpath "$work/trusted" --checksig "$work/tampered.rpm"; then
    echo "Unexpected acceptance of modified RPM"
    exit 1
fi
sha256sum "$signed"
echo "RPM signature fixture passed: trusted accepted; unknown key and modified RPM rejected"
