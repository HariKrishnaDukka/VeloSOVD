#!/usr/bin/env bash
set -euo pipefail
umask 077

ROOT="pki/dev"
SERVER_CA="$ROOT/server-ca"
CLIENT_CA="$ROOT/client-ca"
CENTRAL="$ROOT/central"
HARNESS="$ROOT/harness"
FRONT="$ROOT/zones/front"
REAR="$ROOT/zones/rear"
UNTRUSTED_CA="$ROOT/untrusted-ca"
UNTRUSTED="$ROOT/untrusted"
WORK="$ROOT/work"

mkdir -p "$SERVER_CA" "$CLIENT_CA" "$CENTRAL" "$HARNESS" \
    "$FRONT" "$REAR" "$UNTRUSTED_CA" "$UNTRUSTED" "$WORK"

generate_ca() {
    local name="$1" dir="$2"

    openssl genpkey -algorithm EC \
        -pkeyopt ec_paramgen_curve:P-256 \
        -out "$dir/ca.key"

    openssl req -new -x509 -key "$dir/ca.key" \
        -sha256 -days 3650 \
        -subj "/O=VeloSOVD Development/CN=$name" \
        -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
        -addext "keyUsage=critical,keyCertSign,cRLSign" \
        -out "$dir/ca.crt"

    chmod 600 "$dir/ca.key"
    chmod 644 "$dir/ca.crt"
}

generate_leaf() {
    local name="$1" dir="$2" usage="$3" san="$4"
    local ca_cert="$5" ca_key="$6"

    openssl genpkey -algorithm EC \
        -pkeyopt ec_paramgen_curve:P-256 \
        -out "$dir/$name.key"

    openssl req -new -key "$dir/$name.key" \
        -subj "/O=VeloSOVD Development/CN=$name" \
        -out "$WORK/$name.csr"

    cat > "$WORK/$name.ext" <<EXT
[v3_leaf]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=$usage
subjectAltName=$san
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid,issuer
EXT

    openssl x509 -req -in "$WORK/$name.csr" \
        -CA "$ca_cert" -CAkey "$ca_key" -CAcreateserial \
        -days 365 -sha256 \
        -extfile "$WORK/$name.ext" -extensions v3_leaf \
        -out "$dir/$name.crt"

    chmod 600 "$dir/$name.key"
    chmod 644 "$dir/$name.crt"
}

echo "[PKI] Creating independent Server CA and Client CA"

# Each invocation creates new CA keys. Never reuse serial files
# or leaf certificates issued by a previous CA generation.
rm -f "$SERVER_CA/ca.srl" "$CLIENT_CA/ca.srl"
rm -f "$UNTRUSTED_CA/ca.srl"
rm -f "$CENTRAL"/*.crt "$CENTRAL"/*.key
rm -f "$HARNESS"/*.crt "$HARNESS"/*.key
rm -f "$FRONT"/*.crt "$FRONT"/*.key
rm -f "$REAR"/*.crt "$REAR"/*.key
rm -f "$UNTRUSTED"/*.crt "$UNTRUSTED"/*.key

generate_ca "VeloSOVD Server CA" "$SERVER_CA"
generate_ca "VeloSOVD Client CA" "$CLIENT_CA"

# Initialize independent serial counters for both CAs.
printf '%s\n' 1000 > "$SERVER_CA/ca.srl"
printf '%s\n' 2000 > "$CLIENT_CA/ca.srl"

echo "[PKI] Issuing Central client identity"
generate_leaf central-client "$CENTRAL" clientAuth \
    "DNS:central-sovd" "$CLIENT_CA/ca.crt" "$CLIENT_CA/ca.key"

echo "[PKI] Issuing dedicated harness client identity"
generate_leaf harness-client "$HARNESS" clientAuth \
    "DNS:harness-client" "$CLIENT_CA/ca.crt" "$CLIENT_CA/ca.key"

echo "[PKI] Issuing expired harness client identity"

mkdir -p "$WORK/expired-ca/newcerts"
touch "$WORK/expired-ca/index.txt"

openssl genpkey -algorithm EC \
    -pkeyopt ec_paramgen_curve:P-256 \
    -out "$HARNESS/expired-client.key"

openssl req -new -key "$HARNESS/expired-client.key" \
    -subj "/O=VeloSOVD Development/CN=expired-client" \
    -out "$WORK/expired-client.csr"

cat > "$WORK/expired-ca.cnf" <<EOF_EXPIRED
[ ca ]
default_ca = CA_default

[ CA_default ]
dir = $WORK/expired-ca
database = $WORK/expired-ca/index.txt
new_certs_dir = $WORK/expired-ca/newcerts
certificate = $CLIENT_CA/ca.crt
private_key = $CLIENT_CA/ca.key
serial = $CLIENT_CA/ca.srl
default_md = sha256
default_days = 365
rand_serial = no
policy = policy_any
unique_subject = no
email_in_dn = no
x509_extensions = expired_leaf

[ policy_any ]
commonName = supplied
organizationName = optional

[ expired_leaf ]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = clientAuth
subjectAltName = DNS:expired-client
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid,issuer
EOF_EXPIRED

openssl ca -batch -notext \
    -config "$WORK/expired-ca.cnf" \
    -in "$WORK/expired-client.csr" \
    -out "$HARNESS/expired-client.crt" \
    -startdate 20200101000000Z \
    -enddate 20210101000000Z

chmod 600 "$HARNESS/expired-client.key"
chmod 644 "$HARNESS/expired-client.crt"

echo "[PKI] Creating unrelated Untrusted CA"
generate_ca "VeloSOVD Untrusted Test CA" "$UNTRUSTED_CA"

echo "[PKI] Issuing untrusted client identity"
generate_leaf untrusted-client "$UNTRUSTED" clientAuth \
    "DNS:untrusted-client" \
    "$UNTRUSTED_CA/ca.crt" "$UNTRUSTED_CA/ca.key"

echo "[PKI] Issuing FRONT server identity"
generate_leaf front-server "$FRONT" serverAuth \
    "IP:10.77.10.2" "$SERVER_CA/ca.crt" "$SERVER_CA/ca.key"

echo "[PKI] Issuing REAR server identity"
generate_leaf rear-server "$REAR" serverAuth \
    "IP:10.77.10.6" "$SERVER_CA/ca.crt" "$SERVER_CA/ca.key"

echo "[PKI] Issuing Central HTTPS server identity"
generate_leaf central-server "$CENTRAL" serverAuth \
    "IP:10.77.10.1" "$SERVER_CA/ca.crt" "$SERVER_CA/ca.key"

echo "[PKI] Verifying certificate chains and purposes"

openssl verify -CAfile "$CLIENT_CA/ca.crt" \
    -purpose sslclient "$CENTRAL/central-client.crt"

openssl verify -CAfile "$CLIENT_CA/ca.crt" \
    -purpose sslclient "$HARNESS/harness-client.crt"

openssl verify -CAfile "$SERVER_CA/ca.crt" \
    -purpose sslserver -verify_ip 10.77.10.1 \
    "$CENTRAL/central-server.crt"

if openssl x509 -in "$HARNESS/expired-client.crt" -checkend 0; then
    echo "[PKI] ERROR: Expired client certificate is still valid"
    exit 1
fi

echo "[PKI] Expired client certificate correctly identified"


openssl verify -CAfile "$SERVER_CA/ca.crt" \
    -purpose sslserver -verify_ip 10.77.10.2 \
    "$FRONT/front-server.crt"

openssl verify -CAfile "$SERVER_CA/ca.crt" \
    -purpose sslserver -verify_ip 10.77.10.6 \
    "$REAR/rear-server.crt"

echo "[PKI] Verifying cross-CA rejection"
if openssl verify -CAfile "$CLIENT_CA/ca.crt" \
    -purpose sslclient "$UNTRUSTED/untrusted-client.crt"; then
    echo "[PKI] ERROR: Client CA unexpectedly trusts untrusted client"
    exit 1
fi


if openssl verify -CAfile "$SERVER_CA/ca.crt" \
    -purpose sslclient "$CENTRAL/central-client.crt"; then
    echo "[PKI] ERROR: Server CA unexpectedly trusts Central client certificate"
    exit 1
fi

if openssl verify -CAfile "$CLIENT_CA/ca.crt" \
    -purpose sslserver "$FRONT/front-server.crt"; then
    echo "[PKI] ERROR: Client CA unexpectedly trusts FRONT server certificate"
    exit 1
fi

rm -rf "$WORK"

# Remove obsolete shared-CA material only after successful verification.
rm -rf "$ROOT/offline-ca"
rm -f "$ROOT/ca.crt" "$ROOT/ca.srl"

echo "[PKI] Separate CA generation and cross-CA rejection checks passed"
