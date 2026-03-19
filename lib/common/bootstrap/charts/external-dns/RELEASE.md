### Added

- Add option to set `annotationPrefix` ([#5889](https://github.com/kubernetes-sigs/external-dns/pull/5889)) _@lexfrei_

### Changed

- Grant `networking.k8s.io/ingresses` and `gateway.solo.io/gateways` permissions when using `gloo-proxy` source. ([#5909](https://github.com/kubernetes-sigs/external-dns/pull/5909)) _@cucxabong_
- Update _ExternalDNS_ OCI image version to [v0.20.0](https://github.com/kubernetes-sigs/external-dns/releases/tag/v0.20.0). ([#6005](https://github.com/kubernetes-sigs/external-dns/pull/6005)) _@vflaux_

### Fixed

- Fixed the missing schema for `.provider.webhook.serviceMonitor` configs ([#5932](https://github.com/kubernetes-sigs/external-dns/pull/5932)) _@chrisbsmith_
- Fixed incorrect indentation of selector labels under `spec.template.spec.topologySpreadConstraints` when `topologySpreadConstraints` is set. ([#6054](https://github.com/kubernetes-sigs/external-dns/pull/6054)) _@andylim0221_
