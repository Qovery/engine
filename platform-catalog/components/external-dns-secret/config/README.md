# external-dns-secret

This bundle creates the Qovery DNS API key Secret in `kube-system`. The real cluster JWT is
resolved by q-core in memory and never enters the public catalog artifact.

Slice 4.8 supports the `pdns` provider only. The `dns.providerKind` input is a required fail-closed
provider gate even though the reviewed Helm value is statically `pdns`.
