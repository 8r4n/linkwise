#!/usr/bin/env bash
set -euo pipefail

mkdir "$1"
(
  cd "$1"
  umask 077
  openssl req -x509 -newkey rsa:2048 -nodes -keyout ca.key -out ca.pem \
    -days 7 -subj '/CN=Linkwise Simulator Bench CA' \
    -addext 'basicConstraints=critical,CA:TRUE' \
    -addext 'keyUsage=critical,keyCertSign,cRLSign'
  openssl req -new -newkey rsa:2048 -nodes -keyout server.key \
    -out server.csr -subj '/CN=localhost'
  printf '%s\n' 'subjectAltName=DNS:localhost' 'extendedKeyUsage=serverAuth' \
    'keyUsage=critical,digitalSignature,keyEncipherment' > server.ext
  openssl x509 -req -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial \
    -out server.pem -days 7 -extfile server.ext
)
