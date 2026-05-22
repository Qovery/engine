### Added

- Add value `.sourceNamespace` to watch a namespace which is different from the one that external-dns is installed into when `.namespaced` is true. ([#6297](https://github.com/kubernetes-sigs/external-dns/pull/6297)) _@jplitza_
- Add option to enable Gateway API ListenerSet support. ([#6381](https://github.com/kubernetes-sigs/external-dns/pull/6381)) _@speer_
- Add support for bool in extraArgs. ([#6179](https://github.com/kubernetes-sigs/external-dns/pull/6179)) _@farodin91_
- Add value `.namespaceOverride` to render chart resources into a namespace different from the release namespace, for subchart installs that want their own namespace. ([#6389](https://github.com/kubernetes-sigs/external-dns/pull/6389)) _@alliasgher_

### Changed

- Update _ExternalDNS_ OCI image version to [v0.21.0](https://github.com/kubernetes-sigs/external-dns/releases/tag/v0.21.0). ([#6354](https://github.com/kubernetes-sigs/external-dns/pull/6354)) _@vflaux_

### Fixed

- Avoid creating cluster-scoped RBAC for Gateway API sources when running namespaced with `gatewayNamespace` set. Namespace listing permissions are now only added when `gatewayNamespace` is unset. ([#5843](https://github.com/kubernetes-sigs/external-dns/pull/5843)) _@TobyTheHutt_
- Ensure container arguments are passed in as strings ([#6264](https://github.com/kubernetes-sigs/external-dns/pull/6264)) _@KhooHaoYit_
- Ensure container arguments are passed in as strings when extraArgs is a map ([#6284](https://github.com/kubernetes-sigs/external-dns/pull/6284)) _@vflaux_
