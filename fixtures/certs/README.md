# TLS assets generation

`cfssl` JSON configuration:
 * CA generation: `ca.json`
 * Peer signed certificate: `swarm-peer.json`

Final assets:
 * CA certificate: `ca.pem`
 * Peer certificate and key: `swarm-peer.p12`

# Steps

```sh
# Generate CA
cfssl genkey --initca=true ca.json | cfssljson -bare ca

# Generate peer key
cfssl genkey swarm-peer.json | cfssljson -bare swarm-peer

# Sign peer certificate
cfssl sign -ca ca.pem -ca-key ca-key.pem -hostname mitch-rs swarm-peer.csr | cfssljson -bare swarm-peer

# Export PKCS#12 with a blank password
openssl pkcs12 -export -out swarm-peer.p12 -inkey swarm-peer-key.pem -in swarm-peer.pem -passout pass:
```
