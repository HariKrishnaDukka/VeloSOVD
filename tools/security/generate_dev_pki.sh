#!/usr/bin/env bash
set -euo pipefail
umask 077

ROOT="pki/dev"

CA_DIR="$ROOT/offline-ca"
CENTRAL_DIR="$ROOT/central"
FRONT_DIR="$ROOT/zones/front"
REAR_DIR="$ROOT/zones/rear"
WORK="$ROOT/work"

mkdir -p "$CA_DIR" "$CENTRAL_DIR" "$FRONT_DIR" "$REAR_DIR" "$WORK"

echo "[PKI] Generating development CA..."

openssl genpkey \
    -algorithm EC \
    -pkeyopt ec_paramgen_curve:P-256 \
    -out "$CA_DIR/ca.key"

openssl req -new -x509 \
    -key "$CA_DIR/ca.key" \
    -sha256 \
    -days 3650 \
    -subj "/O=VeloSOVD Development/CN=VeloSOVD Development CA" \
    -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" \
    -addext "subjectKeyIdentifier=hash" \
    -out "$ROOT/ca.crt"

generate_identity() {
    local name="$1"
    local directory="$2"
    local usage="$3"
    local san="$4"

    echo "[PKI] Generating $name identity..."

    openssl genpkey \
        -algorithm EC \
        -pkeyopt ec_paramgen_curve:P-256 \
        -out "$directory/$name.key"

    openssl req -new \
        -key "$directory/$name.key" \
        -subj "/O=VeloSOVD Development/CN=$name" \
        -out "$WORK/$name.csr"

    cat > "$WORK/$name.ext" <<EOF
[$usage]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=$usage
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid,issuer
subjectAltName=$san
EOF

    openssl x509 -req \
        -in "$WORK/$name.csr" \
        -CA "$ROOT/ca.crt" \
        -CAkey "$CA_DIR/ca.key" \
        -CAcreateserial \
        -days 365 \
        -sha256 \
        -extfile "$WORK/$name.ext" \
        -extensions "$usage" \
        -out "$directory/$name.crt"

    chmod 600 "$directory/$name.key"
}

generate_identity \
    "central-client" \
    "$CENTRAL_DIR" \
    "clientAuth" \
    "DNS:central-sovd"

generate_identity \
    "front-server" \
    "$FRONT_DIR" \
    "serverAuth" \
    "IP:10.77.10.2"

generate_identity \
    "rear-server" \
    "$REAR_DIR" \
    "serverAuth" \
    "IP:10.77.10.6"

rm -rf "$WORK"

chmod 600 "$CA_DIR/ca.key"

echo
echo "[PKI] Development certificates generated successfully."
echo "[PKI] CA certificate: $ROOT/ca.crt"
echo "[PKI] CA private key: $CA_DIR/ca.key"
echo "[PKI] Central identity: $CENTRAL_DIR/"
echo "[PKI] FRONT identity: $FRONT_DIR/"
echo "[PKI] REAR identity: $REAR_DIR/"
