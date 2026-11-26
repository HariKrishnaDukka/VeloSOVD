#!/usr/bin/env bash
# M1: development keys for user accounts. Does NOT regenerate the existing PKI.
#
# Creates (from the existing dev Client CA):
#   pki/dev/auth/token-signing.key   P-256 PKCS#8 private key (auth-service only)
#   pki/dev/auth/token-signing.pub   matching public key (central: SDV_TOKEN_PUBLIC_KEY)
#   pki/dev/studio/velosovd-studio.crt/.key   device certificate of the Studio tool
#                                             (DNS:velosovd-studio, clientAuth)
#
# Run from the repository root:  bash tools/security/generate_auth_keys.sh
# Development only: production signing keys belong in an HSM.
set -euo pipefail
umask 077

ROOT="pki/dev"
CLIENT_CA="$ROOT/client-ca"
AUTH="$ROOT/auth"
STUDIO="$ROOT/studio"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

for f in "$CLIENT_CA/ca.crt" "$CLIENT_CA/ca.key"; do
    [[ -r "$f" ]] || { echo "ERROR: $f not readable (run generate_dev_pki.sh first, or use sudo)"; exit 1; }
done
mkdir -p "$AUTH" "$STUDIO"

echo "[AUTH-KEYS] Token signing key pair (ES256 / P-256)"
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$AUTH/token-signing.key"
openssl pkey -in "$AUTH/token-signing.key" -pubout -out "$AUTH/token-signing.pub"
chmod 600 "$AUTH/token-signing.key"
chmod 644 "$AUTH/token-signing.pub"

echo "[AUTH-KEYS] Studio device certificate (DNS:velosovd-studio)"
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$STUDIO/velosovd-studio.key"
openssl req -new -key "$STUDIO/velosovd-studio.key" \
    -subj "/O=VeloSOVD Development/CN=velosovd-studio" -out "$WORK/studio.csr"
cat > "$WORK/studio.ext" <<EXT
[v3_leaf]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=clientAuth
subjectAltName=DNS:velosovd-studio
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid,issuer
EXT
openssl x509 -req -in "$WORK/studio.csr" \
    -CA "$CLIENT_CA/ca.crt" -CAkey "$CLIENT_CA/ca.key" -CAcreateserial \
    -days 365 -sha256 -extfile "$WORK/studio.ext" -extensions v3_leaf \
    -out "$STUDIO/velosovd-studio.crt" 2>/dev/null
chmod 600 "$STUDIO/velosovd-studio.key"
chmod 644 "$STUDIO/velosovd-studio.crt"

openssl verify -CAfile "$CLIENT_CA/ca.crt" -purpose sslclient "$STUDIO/velosovd-studio.crt"

# When run with sudo, hand the files back to the calling user.
if [[ -n "${SUDO_USER:-}" ]]; then
    chown -R "$SUDO_USER" "$AUTH" "$STUDIO"
fi
echo "[AUTH-KEYS] Done: $AUTH/token-signing.{key,pub}, $STUDIO/velosovd-studio.{crt,key}"
