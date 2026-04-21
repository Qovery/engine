## Release notes engine v1.258.1   
### Bug fixes   
* [c4a32729a098d314695032431904261d6651c1c6](https://github.com/Qovery/engine/commit/c4a32729a098d314695032431904261d6651c1c6): fix(aws): correctly escape annotation value  (Romain GERARD)
## Release notes engine v1.258.0   
### Others   
* [c320b6f1a1c15bcddc09d6030af32079b4a79751](https://github.com/Qovery/engine/commit/c320b6f1a1c15bcddc09d6030af32079b4a79751): feat(QOV-1852): add automatic AMI refresh for karpenter-controller nodegroup  (Guillaume Dubroeucq)
## Release notes engine v1.257.0   
### Features   
* [716bd894099153ac4d8141b9212cad65e4f1f1f3](https://github.com/Qovery/engine/commit/716bd894099153ac4d8141b9212cad65e4f1f1f3): feat: Block SSE-C following aws reco  (Melvin Zottola)
## Release notes engine v1.256.1   
### Others   
* [d47804301f59759636cdd5082264679acbb69c73](https://github.com/Qovery/engine/commit/d47804301f59759636cdd5082264679acbb69c73): chore(aws-lb): remove default aws-load-balancer-healthcheck-interval  (Σrebe - Romain GERARD)
## Release notes engine v1.256.0   
### Features   
* [feed1df72115d9f43b6c2cb80c533ffeecd54625](https://github.com/Qovery/engine/commit/feed1df72115d9f43b6c2cb80c533ffeecd54625): feat(aws): remove default aws-load-balancer-healthcheck-interval LB annotations  (Romain GERARD)
## Release notes engine v1.255.0   
### Features   
* [9196521dd03d0623e21872de856f6680ae651115](https://github.com/Qovery/engine/commit/9196521dd03d0623e21872de856f6680ae651115): feat(lb): support restricting ingress traffic at AWS LB lvl  (Σrebe - Romain GERARD)
## Release notes engine v1.254.3   
### Bug fixes   
* [d628c5cb1aac13987f35f3a2127b16723c512ca0](https://github.com/Qovery/engine/commit/d628c5cb1aac13987f35f3a2127b16723c512ca0): fix(terraform): stabilize S3 SSE config for AWS clusters to remove recurring drift  (Pierre Gerbelot)
   
### Others   
* [9ac538fa4342e764072a104dfb3ff1ba9340596f](https://github.com/Qovery/engine/commit/9ac538fa4342e764072a104dfb3ff1ba9340596f): chore(QOV-1832): bump kubectl binary to 1.34.7  (Guillaume Dubroeucq)
## Release notes engine v1.254.2   
### Others   
* [7e4d5ba178dfd168d0a4fc3a79bd6ec9d445d5c0](https://github.com/Qovery/engine/commit/7e4d5ba178dfd168d0a4fc3a79bd6ec9d445d5c0): fix(QOV-1837): increase Scaleway node group readiness timeout from 20min to 45min  (Guillaume Dubroeucq)
   
* [4ca48a6df0da1202e1ec9eaedd03452ffc5767ba](https://github.com/Qovery/engine/commit/4ca48a6df0da1202e1ec9eaedd03452ffc5767ba): fix(eks-anywhere): improve flow  (Pierre Gerbelot)
## Release notes engine v1.254.1   
### Others   
* [9c3dcdb78125fba6e4afbece2e1465274a149713](https://github.com/Qovery/engine/commit/9c3dcdb78125fba6e4afbece2e1465274a149713): Revert "disable go releaser"  (Σrebe - Romain GERARD)
## Release notes engine v1.250.0   
### Features   
* [ff64094130ad47ddd68977715e81cf58db48d98c](https://github.com/Qovery/engine/commit/ff64094130ad47ddd68977715e81cf58db48d98c): feat: add Kubernetes recommended labels (app.kubernetes.io/*) without version  (Guillaume Da Silva)
   
### Bug fixes   
* [70d56dceecf36abb41f813160d6e233702844ad5](https://github.com/Qovery/engine/commit/70d56dceecf36abb41f813160d6e233702844ad5): fix: Fix missing external secret on test  (Melvin Zottola)
   
### Others   
* [ce7be9dc20fce174fcffac2629658862a32114ae](https://github.com/Qovery/engine/commit/ce7be9dc20fce174fcffac2629658862a32114ae): feat(QOV-1820): envoy allow to set maxStreamDuration  (benjaminch)
   
* [3bcca01095f1057021cf5ce14c92157971d72a0c](https://github.com/Qovery/engine/commit/3bcca01095f1057021cf5ce14c92157971d72a0c): feat(QOV-1820): envoy allow to set stream_idle_timeout  (benjaminch)
   
* [607ff4afea5a979d3a1ba6e738beb0cb2f4b3e17](https://github.com/Qovery/engine/commit/607ff4afea5a979d3a1ba6e738beb0cb2f4b3e17): test(QOV-1179): add EFS addon integration test for EKS cluster  (Guillaume Da Silva)
## Release notes engine v1.249.0   
### Others   
* [4b18e1d15f8bedd54531c527cfa6ed71064e504c](https://github.com/Qovery/engine/commit/4b18e1d15f8bedd54531c527cfa6ed71064e504c): feat(QOV-794): add dedicated Karpenter cronjob nodepool for AWS EKS  (Guillaume Da Silva)
## Release notes engine v1.248.0   
### Bug fixes   
* [5eab707678252dca52fd0bef7c8500f865239540](https://github.com/Qovery/engine/commit/5eab707678252dca52fd0bef7c8500f865239540): fix(Helm): Do not allow empty namespace and add rollback tag  (Antoine Promerova)
   
### Others   
* [4343f51253460cdde1ecfeceb04c51085c0d6553](https://github.com/Qovery/engine/commit/4343f51253460cdde1ecfeceb04c51085c0d6553): feat(QOV-1179): add aws.eks.enable_efs_addon advanced setting with EFS CSI driver add-on  (Guillaume Da Silva)
## Release notes engine v1.247.0   
### Others   
* [008a078f4f045508e85ac36349b9454212abdca6](https://github.com/Qovery/engine/commit/008a078f4f045508e85ac36349b9454212abdca6): feat(qov-1567) Support external secrets for app, container, job  (Melvin Zottola)
## Release notes engine v1.246.1   
### Others   
* [345c567c395f8ad22a8676cf16d975fe2886d2bf](https://github.com/Qovery/engine/commit/345c567c395f8ad22a8676cf16d975fe2886d2bf): chore(QOV-1777): release gateway-api FF for deduplicate routes  (benjaminch)
## Release notes engine v1.246.0   
### Internal changes   
* [66f8bfc9f016bfdd9b056bdbeabeb9d9cc1a0ddb](https://github.com/Qovery/engine/commit/66f8bfc9f016bfdd9b056bdbeabeb9d9cc1a0ddb): chore(registry): Add ecr proxy registry for gcp clusters  (Antoine Promerova)
   
### Others   
* [d2fdcb75aecf61c27de84a6edcdbbf4d29cfc223](https://github.com/Qovery/engine/commit/d2fdcb75aecf61c27de84a6edcdbbf4d29cfc223): fix(QOV-1796): enhance error handling checks for admission policy denial  (Guillaume Dubroeucq)
## Release notes engine v1.245.1   
### Others   
* [c4570a93f6ffd32392ac9d437a561dd012f13e43](https://github.com/Qovery/engine/commit/c4570a93f6ffd32392ac9d437a561dd012f13e43): chore(QOV-1777): gateway-api deduplicate routes  (Pierre Gerbelot)
## Release notes engine v1.245.0   
### Features   
* [83c7d3e634ca19aa7e9887d6382baf6f57d38998](https://github.com/Qovery/engine/commit/83c7d3e634ca19aa7e9887d6382baf6f57d38998): feat(eksanywhere): fetch cluster yaml from git and run upgrade plan on updates  (Pierre Gerbelot)
## Release notes engine v1.244.3   
### Others   
* [a1dbc57beb4b929eaf902f9b0b39c88c3e073fd3](https://github.com/Qovery/engine/commit/a1dbc57beb4b929eaf902f9b0b39c88c3e073fd3): fix(QOV-1774): Gateway-API fix URL prefix declaration v2  (benjaminch)
## Release notes engine v1.244.2   
### Others   
* [ac2000d4946ad9939354d0cda52063e15ae38613](https://github.com/Qovery/engine/commit/ac2000d4946ad9939354d0cda52063e15ae38613): fix(eks-anywhere): change metallb image and disable gateway  (Pierre Gerbelot)
## Release notes engine v1.244.1   
### Bug fixes   
* [83030c6c34d4915f2d621238b5b4b5cbb06c02e6](https://github.com/Qovery/engine/commit/83030c6c34d4915f2d621238b5b4b5cbb06c02e6): fix(chart): increase ram for engine env to 3Gi  (benjaminch)
   
### Others   
* [9a071a3e430abfe415e98a294448257df56c510d](https://github.com/Qovery/engine/commit/9a071a3e430abfe415e98a294448257df56c510d): chore(QOV-1772): Envoy not to block whole request if header has _  (benjaminch)
   
* [c72def04786f4041018f9828b2c5bf1400ef9223](https://github.com/Qovery/engine/commit/c72def04786f4041018f9828b2c5bf1400ef9223): fix(QOV-1774): Gateway-API fix URL prefix declaration  (benjaminch)
## Release notes engine v1.244.0   
### Others   
* [3a00db37083680a3de16b5ce90482af51e1925a4](https://github.com/Qovery/engine/commit/3a00db37083680a3de16b5ce90482af51e1925a4): feat(QOV-1729): add lifecycle ignore_changes for customer-managed AWS DB attributes  (Fabien FLEUREAU)
## Release notes engine v1.243.1   
### Bug fixes   
* [52f3da5fb3f60450e38b0ed230974dc077d0433c](https://github.com/Qovery/engine/commit/52f3da5fb3f60450e38b0ed230974dc077d0433c): fix: version MySQL parameter group name to support version upgrades  (Guillaume Da Silva)
## Release notes engine v1.243.0   
### Others   
* [1c2ef34c038e22e5248fa365e45c90762b5c2081](https://github.com/Qovery/engine/commit/1c2ef34c038e22e5248fa365e45c90762b5c2081): feat(QOV-1715): detect buildkit pod crashes and fail immediately  (Fabien FLEUREAU)
## Release notes engine v1.242.0   
### Others   
* [3d2f194cb8aa9954feceed2e2dc991824b8a5da0](https://github.com/Qovery/engine/commit/3d2f194cb8aa9954feceed2e2dc991824b8a5da0): feat(QOV-1767): add Karpenter migration safety check before removing old autoscaler  (Guillaume Dubroeucq)
   
* [aed09187f649f327b28f0d043c1c11efda10660d](https://github.com/Qovery/engine/commit/aed09187f649f327b28f0d043c1c11efda10660d): fix(QOV-1766): Gateway API properly attribute port per domain  (benjaminch)
## Release notes engine v1.241.1   
### Bug fixes   
* [e77e39d18d4c904525db835240a7b01a32ec9749](https://github.com/Qovery/engine/commit/e77e39d18d4c904525db835240a7b01a32ec9749): fix: truncate app.kubernetes.io/version to 63 chars (K8s label limit)  (Guillaume Da Silva)
   
### Others   
* [b1069c0badccb63d77365e2f56309719fdf7849e](https://github.com/Qovery/engine/commit/b1069c0badccb63d77365e2f56309719fdf7849e): revert: remove app.kubernetes.io/* labels (reverting !2439 and !2444)  (Guillaume Da Silva)
## Release notes engine v1.241.0   
### Features   
* [2accaf91deb407e7443076a8fe66bbf5f72a1bd9](https://github.com/Qovery/engine/commit/2accaf91deb407e7443076a8fe66bbf5f72a1bd9): feat: add Kubernetes recommended labels (app.kubernetes.io/*) to all workload resources  (Guillaume Da Silva)
   
### Others   
* [e14ec2a6cd81ba2515e1423b6d30b2843ac4e382](https://github.com/Qovery/engine/commit/e14ec2a6cd81ba2515e1423b6d30b2843ac4e382): feat(QOV-1688): add Kubernetes 1.34 version support  (Guillaume Dubroeucq)
## Release notes engine v1.240.0   
### Bug fixes   
* [35757d3c59eb70d49b6fec3a5071099468a76dfd](https://github.com/Qovery/engine/commit/35757d3c59eb70d49b6fec3a5071099468a76dfd): fix(clippy): resolve test warnings and add dynamic feature-matrix lint  (Pierre Gerbelot)
   
### Others   
* [72c51c891025ea06dda5b255c1dc75c2ca2886b4](https://github.com/Qovery/engine/commit/72c51c891025ea06dda5b255c1dc75c2ca2886b4): feat(QOV-1682): allow cluster to read envoy object  (Pierre Gerbelot)
   
* [ffa7adba7b8fc6479cba766cfb8c31781d48851e](https://github.com/Qovery/engine/commit/ffa7adba7b8fc6479cba766cfb8c31781d48851e): fix(QOV-1762): GKE ListenersSet not authorized  (benjaminch)
   
* [a38e63cfc00563af12e2f0ac6640b6b42f9d93eb](https://github.com/Qovery/engine/commit/a38e63cfc00563af12e2f0ac6640b6b42f9d93eb): fix(QOV-1762): GKE cluster update not to erase gateway certificates  (benjaminch)
   
* [ec910dff6ebc74c3959fb567469b382b62879bc4](https://github.com/Qovery/engine/commit/ec910dff6ebc74c3959fb567469b382b62879bc4): fix(qov-1564) Use generic name for cluster secret store  (Melvin Zottola)
## Release notes engine v1.239.0   
### Others   
* [7e2b65876b48eede47a3aaf065d7dc6b4fdfdd7c](https://github.com/Qovery/engine/commit/7e2b65876b48eede47a3aaf065d7dc6b4fdfdd7c): feat(QOV-1756): add GATEWAY_API_ROUTES annotation scope for HTTPRoute and GRPCRoute  (Pierre Gerbelot)
## Release notes engine v1.238.1   
### Others   
* [a90936dfeb553016e46359cf0f61c39f35668af8](https://github.com/Qovery/engine/commit/a90936dfeb553016e46359cf0f61c39f35668af8): chore(QOV-1754): force update cert owner when switching to envoy  (benjaminch)
## Release notes engine v1.238.0   
### Others   
* [eac20f79fea8fbeb842c25c9a2008beaa345c3c0](https://github.com/Qovery/engine/commit/eac20f79fea8fbeb842c25c9a2008beaa345c3c0): feat(QOV-1635): enable check on subnet provided by the client only for AWS  (Pierre Gerbelot)
## Release notes engine v1.237.0   
### Others   
* [8ab3cd30ab0c547b1bb78b43b5f991f903270f61](https://github.com/Qovery/engine/commit/8ab3cd30ab0c547b1bb78b43b5f991f903270f61): feat(QOV-1635): enable check on AWS subnet tag on custom VPC whatever the value of alb controller  (Pierre Gerbelot)
## Release notes engine v1.236.0   
### Others   
* [40f75837adb661f7fd0149788222e848c5ac5c79](https://github.com/Qovery/engine/commit/40f75837adb661f7fd0149788222e848c5ac5c79): feat(QOV-1392): enable engine post-renderer labels for all clusters  (Pierre Gerbelot)
## Release notes engine v1.235.1   
### Others   
* [8cbc963a0d223716cfc9f9b761c4adc1d5a0cc57](https://github.com/Qovery/engine/commit/8cbc963a0d223716cfc9f9b761c4adc1d5a0cc57): fix(QOV-1739): gateway-api add a dedicated route for acme challenge  (benjaminch)
## Release notes engine v1.235.0   
### Others   
* [c2bc3b65d399faf19f825b1ac9c6c46e8b2c8ce7](https://github.com/Qovery/engine/commit/c2bc3b65d399faf19f825b1ac9c6c46e8b2c8ce7): feat(COR-1732): verify gateway exists and readiness across gateway/envoy charts  (Pierre Gerbelot)
## Release notes engine v1.234.0   
### Bug fixes   
* [c841137a7f1d1814ebe1596129c86f1da9bfaeb2](https://github.com/Qovery/engine/commit/c841137a7f1d1814ebe1596129c86f1da9bfaeb2): fix(helm): prevent traffic_policy template failure when numberTrustedHops is null  (Pierre Gerbelot)
   
### Others   
* [9aef75b7a61053b0313dd90ed90b057ea096287b](https://github.com/Qovery/engine/commit/9aef75b7a61053b0313dd90ed90b057ea096287b): chore(QOV-1689): update Karpenter to version 1.10.0 and add capacity reservation interrupt event  (Guillaume Dubroeucq)
   
* [9d04c815f0aa2bbfb39d4651afa09e6f7ac8c816](https://github.com/Qovery/engine/commit/9d04c815f0aa2bbfb39d4651afa09e6f7ac8c816): chore(QOV-1696): upgrade external-dns to version 1.20.0  (Guillaume Dubroeucq)
   
* [0197aed50db15c7c211848e30e9dcc81a8e43eb4](https://github.com/Qovery/engine/commit/0197aed50db15c7c211848e30e9dcc81a8e43eb4): chore(QOV-1697): update promtail version to 3.6.7  (Guillaume Dubroeucq)
   
* [2230f62aca42f5796ab3a107bdc702e17d01c37e](https://github.com/Qovery/engine/commit/2230f62aca42f5796ab3a107bdc702e17d01c37e): feat(QOV-1690): upgrade cluster-autoscaler to v1.34.2  (Guillaume Dubroeucq)
## Release notes engine v1.233.0   
### Features   
* [0f850bcdd704a4d338bddafaab658579172ab7c5](https://github.com/Qovery/engine/commit/0f850bcdd704a4d338bddafaab658579172ab7c5): feat(powens): add support for eksctl anywhere command  (Pierre Gerbelot)
   
* [ecf61ccdc839bab7efceaa17d9ee7a236e6a9f6c](https://github.com/Qovery/engine/commit/ecf61ccdc839bab7efceaa17d9ee7a236e6a9f6c): feat: add deployment.topology_spread.zone advanced setting  (Guillaume Da Silva)
   
### Bug fixes   
* [909d570f7f5ae88ce1e5318e37acc8652c96ad80](https://github.com/Qovery/engine/commit/909d570f7f5ae88ce1e5318e37acc8652c96ad80): fix(powens): add support for eksctl anywhere command  (Pierre Gerbelot)
## Release notes engine v1.232.0   
### Others   
* [85bdfb9e3a632deff18092ab666efb98367df02b](https://github.com/Qovery/engine/commit/85bdfb9e3a632deff18092ab666efb98367df02b): feat(QOV-1714): scw activate proxy protocol v2 for gateway-api  (benjaminch)
## Release notes engine v1.231.0   
### Features   
* [cc54189989d129cfd3d7240e39774ed36896b8a2](https://github.com/Qovery/engine/commit/cc54189989d129cfd3d7240e39774ed36896b8a2): feat(envoy): add data-plane PDB and topology spread constraints to dataplane  (Pierre Gerbelot)
## Release notes engine v1.230.3   
### Others   
* [29b45c62e6c04dcf8f7964f6b28b96160c1eb1d8](https://github.com/Qovery/engine/commit/29b45c62e6c04dcf8f7964f6b28b96160c1eb1d8): fix(gateway-api): wire Envoy dataplane HPA, swap hpa settings mapping, and fix...  (Pierre Gerbelot)
## Release notes engine v1.230.2   
### Bug fixes   
* [34436ac7abc23b55ad3912e4d159e3a5d239ab51](https://github.com/Qovery/engine/commit/34436ac7abc23b55ad3912e4d159e3a5d239ab51): fix: nginx hostname annotation  (benjaminch)
   
### Others   
* [00019e38b2688bf3692b8ccb4ef83e5484ac6438](https://github.com/Qovery/engine/commit/00019e38b2688bf3692b8ccb4ef83e5484ac6438): fix(qov-1564) Uninstall properly eso on gke  (Melvin Zottola)
## Release notes engine v1.230.1   
### Others   
* [cbbcb30226b14b77f248a424016adaabcbf859a4](https://github.com/Qovery/engine/commit/cbbcb30226b14b77f248a424016adaabcbf859a4): fix(QOV-1720): aws remove gateway node affinity for non karpenter clusters  (benjaminch)
   
* [ad5e27091a16dc2039f332d6af30f620b9e91ffa](https://github.com/Qovery/engine/commit/ad5e27091a16dc2039f332d6af30f620b9e91ffa): fix(QOV-1728): fix cluster issuer for envoy dual stack  (benjaminch)
   
* [9601bc3cea5ad88f44d07bee47af34bb24828f01](https://github.com/Qovery/engine/commit/9601bc3cea5ad88f44d07bee47af34bb24828f01): fix(QOV-1730): externaldns to watch services as sources  (benjaminch)
## Release notes engine v1.230.0   
### Others   
* [9921384971c33d2b9bf2122e8e505d5a43c8b4c0](https://github.com/Qovery/engine/commit/9921384971c33d2b9bf2122e8e505d5a43c8b4c0): feat(QOV-635): enable spot instances per nodepool instead of globally  (Guillaume Dubroeucq)
   
* [f4aa113d1e5bb2a408d2e92e4fc89b06511892f5](https://github.com/Qovery/engine/commit/f4aa113d1e5bb2a408d2e92e4fc89b06511892f5): fix(jinja template): Fix rendering not to parse embedded ','  (Antoine)
   
* [363e0e126b62b85015d5978ff1658a5a1d896b4b](https://github.com/Qovery/engine/commit/363e0e126b62b85015d5978ff1658a5a1d896b4b): fix(qov-1564) Uninstall properly external secrets operator  (Melvin Zottola)
## Release notes engine v1.229.0   
### Others   
* [12e5d7b09502780092e61752eda6a006f012f9ea](https://github.com/Qovery/engine/commit/12e5d7b09502780092e61752eda6a006f012f9ea): feat(QOV-1392): add helm post-renderer to inject deployed-by:qovery label  (Pierre Gerbelot)
## Release notes engine v1.228.0   
### Others   
* [5bb29124480d570a118b9fa49435e9560c5696b2](https://github.com/Qovery/engine/commit/5bb29124480d570a118b9fa49435e9560c5696b2): feat(QOV-1712):ignore changes on maintenance window for all managed db  (Laura Millie)
## Release notes engine v1.227.0   
### Others   
* [f1c2ecb8e5ad90897a1d9795b460ae20755176f8](https://github.com/Qovery/engine/commit/f1c2ecb8e5ad90897a1d9795b460ae20755176f8): feat(QOV-1691): remove kube-state-metrics chart  (Guillaume Dubroeucq)
   
* [2bf362b132adf8ba36aee888cab92216e970cc55](https://github.com/Qovery/engine/commit/2bf362b132adf8ba36aee888cab92216e970cc55): feat(QOV-1693): upgrade metrics-server helm chart from 3.12.1 to 3.13.0  (Guillaume Dubroeucq)
   
* [4d42e0c567484604ec9acf313d5a83cdf63ffece](https://github.com/Qovery/engine/commit/4d42e0c567484604ec9acf313d5a83cdf63ffece): feat(QOV-1694): upgrade vertical-pod-autoscaler to 1.6.0  (Guillaume Dubroeucq)
   
* [100624e642c54b4036f73e2cf1c2802295e8598b](https://github.com/Qovery/engine/commit/100624e642c54b4036f73e2cf1c2802295e8598b): feat(QOV-1695): upgrade keda from v2.18.2 to v2.19.0  (Guillaume Dubroeucq)
## Release notes engine v1.226.0   
### Features   
* [21ba0f721a10663aa0ec93c4d67ad83e691f04cc](https://github.com/Qovery/engine/commit/21ba0f721a10663aa0ec93c4d67ad83e691f04cc): feat: document build pod naming convention and add prefix logging  (Guillaume Da Silva)
   
### Others   
* [6da3ffb5e9daf6049683047bca4178bb3abdde15](https://github.com/Qovery/engine/commit/6da3ffb5e9daf6049683047bca4178bb3abdde15): feat(qov-1567) Identify uniquely ClusterSecretStore by kube label  (Melvin Zottola)
## Release notes engine v1.225.0   
### Others   
* [33a3b4021d72863e8d10a8df4c5dab85bb31a9c1](https://github.com/Qovery/engine/commit/33a3b4021d72863e8d10a8df4c5dab85bb31a9c1): feat(QOV-1095): Envoy - Allow to specify default timeouts at cluster level  (Pierre Gerbelot)
   
* [d0311f46a6a20ea5c856d5b63f2d75be23eeb8e8](https://github.com/Qovery/engine/commit/d0311f46a6a20ea5c856d5b63f2d75be23eeb8e8): feat(QOV-1669): add X-Envoy headers  (benjaminch)
## Release notes engine v1.224.1   
### Others   
* [56c7ceb6ee179e9f78afc6f12653f15508da9b44](https://github.com/Qovery/engine/commit/56c7ceb6ee179e9f78afc6f12653f15508da9b44): fix(QOV-1617): fix cluster issuer acme source for envoy default  (benjaminch)
## Release notes engine v1.224.0   
### Others   
* [df071900bc5e70fd9d1497c924523b42e2d983ff](https://github.com/Qovery/engine/commit/df071900bc5e70fd9d1497c924523b42e2d983ff): feat(QOV-1635): enforce strict ALB subnet tag validation on cluster creation, warn-only on updates  (Pierre Gerbelot)
## Release notes engine v1.223.0   
### Bug fixes   
* [0aa227bc8265018a4a5afc86cde26e9f646ef9bf](https://github.com/Qovery/engine/commit/0aa227bc8265018a4a5afc86cde26e9f646ef9bf): fix(pluto): increase the memory allocated to pluto  (Pierre Gerbelot)
   
* [25a5243135da9a30cf65a599930bbb71ea14fb2f](https://github.com/Qovery/engine/commit/25a5243135da9a30cf65a599930bbb71ea14fb2f): fix(tests): rename GCP gateway-api tests names  (benjaminch)
   
### Others   
* [f932afd98aed2d12c055d6849fe08531373fe7c7](https://github.com/Qovery/engine/commit/f932afd98aed2d12c055d6849fe08531373fe7c7): feat(QOV-1617): make sure ListenerSets kind are present to deploy it  (benjaminch)
## Release notes engine v1.222.0   
### Others   
* [9e9c1dc52e96b25daff9290308587628eda0c329](https://github.com/Qovery/engine/commit/9e9c1dc52e96b25daff9290308587628eda0c329): Reapply "feat(QOV-1617): implement xlisteners in routes"  (benjaminch)
## Release notes engine v1.221.0   
### Others   
* [587df57213e43af3cb7cf69c7e4fcf004cbe7c2b](https://github.com/Qovery/engine/commit/587df57213e43af3cb7cf69c7e4fcf004cbe7c2b): Revert "feat(QOV-1617): implement xlisteners in routes"  (benjaminch)
   
* [9ad4eaaa0639645a9596918fa7fe51bb1a0b9549](https://github.com/Qovery/engine/commit/9ad4eaaa0639645a9596918fa7fe51bb1a0b9549): chore(QOV-1689): update Karpenter to 1.9.0  (Guillaume Dubroeucq)
   
* [e4fe150bdc1d1f3fed8d5319e8e6d0a5b566f04c](https://github.com/Qovery/engine/commit/e4fe150bdc1d1f3fed8d5319e8e6d0a5b566f04c): fix(QOV-1611): Prevent cluster deployment from ending up blocked  (Pierre Gerbelot)
## Release notes engine v1.220.0   
### Bug fixes   
* [9f938ea99fa4f81435a06a3fc937580dad1ecabb](https://github.com/Qovery/engine/commit/9f938ea99fa4f81435a06a3fc937580dad1ecabb): fix(tests): make test domain per provider based on cluster URL  (benjaminch)
   
### Others   
* [282611d9e5e0fdcd06aff2d586abf2756b4eca58](https://github.com/Qovery/engine/commit/282611d9e5e0fdcd06aff2d586abf2756b4eca58): feat(QOV-1617): implement xlisteners in routes  (benjaminch)
## Release notes engine v1.219.0   
### Bug fixes   
* [9f938ea99fa4f81435a06a3fc937580dad1ecabb](https://github.com/Qovery/engine/commit/9f938ea99fa4f81435a06a3fc937580dad1ecabb): fix(tests): make test domain per provider based on cluster URL  (benjaminch)
   
### Others   
* [282611d9e5e0fdcd06aff2d586abf2756b4eca58](https://github.com/Qovery/engine/commit/282611d9e5e0fdcd06aff2d586abf2756b4eca58): feat(QOV-1617): implement xlisteners in routes  (benjaminch)
## Release notes engine v1.218.0   
### Others   
* [d352b344b6b7cd118dc63488c013a14cc15103a1](https://github.com/Qovery/engine/commit/d352b344b6b7cd118dc63488c013a14cc15103a1): fix(QOV-1641): Enable API IP whitelist for AKS clusters through API  (Guillaume Dubroeucq)
## Release notes engine v1.217.0   
### Features   
* [ce18879dd7ca3a0e7fe5151546c850465b37fd1f](https://github.com/Qovery/engine/commit/ce18879dd7ca3a0e7fe5151546c850465b37fd1f): feat(powens): rename image to eksanywhere  (Pierre Gerbelot)
   
### Bug fixes   
* [ad8a1fae2b7a23f8d2c8dc123095c2d27a0c1111](https://github.com/Qovery/engine/commit/ad8a1fae2b7a23f8d2c8dc123095c2d27a0c1111): fix: multi arch image creation  (Pierre Gerbelot)
   
### Others   
* [7b4fb839f0c47289151ba4101d49c9307434ed23](https://github.com/Qovery/engine/commit/7b4fb839f0c47289151ba4101d49c9307434ed23): fix(QOV-1521): classify AWS AddressLimitExceeded terraform errors as QuotasExceeded  (Pierre Gerbelot)
## Release notes engine v1.216.0   
### Features   
* [5e5b704840b5faec92b441d419e8ac1b6a843f66](https://github.com/Qovery/engine/commit/5e5b704840b5faec92b441d419e8ac1b6a843f66): feat(powens): add support for eksctl for eks anywhere cluster  (Pierre Gerbelot)
## Release notes engine v1.215.0   
### Others   
* [5fce867c76819ac47fa2782b9ce47d68c273c660](https://github.com/Qovery/engine/commit/5fce867c76819ac47fa2782b9ce47d68c273c660): feat(QOV-1642): add support for eks anywhere git repo and yaml path  (Pierre Gerbelot)
   
* [3c50d394928b2d1ae7aa710b690e9ffd6177b0e6](https://github.com/Qovery/engine/commit/3c50d394928b2d1ae7aa710b690e9ffd6177b0e6): feat(QOV-1644): remove kubent and use pluto  (Pierre Gerbelot)
## Release notes engine v1.214.1   
### Bug fixes   
* [6e0992d49837536ba861b7105f27feb3bece2dff](https://github.com/Qovery/engine/commit/6e0992d49837536ba861b7105f27feb3bece2dff): fix(pluto): improve rule_set output  (Pierre Gerbelot)
## Release notes engine v1.214.0   
### Bug fixes   
* [6ee63ea2c248b92011079fb37adc0f1c7e8edc31](https://github.com/Qovery/engine/commit/6ee63ea2c248b92011079fb37adc0f1c7e8edc31): fix(pluto): treat exit codes 2-4 as deprecated API scan results  (Pierre Gerbelot)
   
### Others   
* [6155a1c0c64e3a3ccb75c3a5c1d8132416d15d10](https://github.com/Qovery/engine/commit/6155a1c0c64e3a3ccb75c3a5c1d8132416d15d10): feat(qov-1622) Add eso cluster outputs  (Melvin Zottola)
   
* [b3cf5106bf328e352b1d2a29d63c627262d956c7](https://github.com/Qovery/engine/commit/b3cf5106bf328e352b1d2a29d63c627262d956c7): feat(qov-1622) Add eso cluster outputs  (Melvin Zottola)
   
* [050652ea7ff2a445bdd461537cd6fae2924c107f](https://github.com/Qovery/engine/commit/050652ea7ff2a445bdd461537cd6fae2924c107f): fix(qov-1564) Fix external secret operator deployment  (Melvin Zottola)
## Release notes engine v1.213.0   
### Features   
* [f0b1f0930d73240346f10d7982e5cdb896579f2d](https://github.com/Qovery/engine/commit/f0b1f0930d73240346f10d7982e5cdb896579f2d): feat: enable test cluster for pluto check  (Pierre Gerbelot)
## Release notes engine v1.212.1   
### Bug fixes   
* [193464895830a01d4cd3d3a96724fe967970ba0d](https://github.com/Qovery/engine/commit/193464895830a01d4cd3d3a96724fe967970ba0d): fix(karpenter): encrypt Bottlerocket second EBS volume (/dev/xvdb) on managed nodegroup  (Guillaume Da Silva)
## Release notes engine v1.212.0   
### Features   
* [3cc115520ed2b306ae79c04a54091dd918e2d456](https://github.com/Qovery/engine/commit/3cc115520ed2b306ae79c04a54091dd918e2d456): feat: add aws.vpc.enable_nat_gateway_secondary_eip cluster advanced setting  (Guillaume Da Silva)
## Release notes engine v1.211.0   
### Internal changes   
* [ea12fdb266b53d3fc3c289cbd040a83f87b5bea9](https://github.com/Qovery/engine/commit/ea12fdb266b53d3fc3c289cbd040a83f87b5bea9): chore: instances-fetcher  (rust-backend-instances-fetcher-pull-request-token)
   
### Others   
* [f9dcca03d846368e05575481fbbeefeed40d97e9](https://github.com/Qovery/engine/commit/f9dcca03d846368e05575481fbbeefeed40d97e9): feat(QOV-1638): add pluto rollout safeguards and deprecation filtering  (Pierre Gerbelot)
## Release notes engine v1.210.0   
### Others   
* [f5e5137c130a976ef5fc8bef0435894d0997ca94](https://github.com/Qovery/engine/commit/f5e5137c130a976ef5fc8bef0435894d0997ca94): feat(QOV-1641): implement API server IP whitelist configuration for AKS  (Guillaume Dubroeucq)
## Release notes engine v1.209.3   
### Bug fixes   
* [38e8d8d3b8a242d12b4ea036b07240605e43fd9f](https://github.com/Qovery/engine/commit/38e8d8d3b8a242d12b4ea036b07240605e43fd9f): fix(scaleway): wait for new node pool to be ready before destroying old one  (Guillaume Da Silva)
## Release notes engine v1.209.2   
### Bug fixes   
* [27aaab9cb8c3a4a0fe3829230658a9d71b88483c](https://github.com/Qovery/engine/commit/27aaab9cb8c3a4a0fe3829230658a9d71b88483c): fix(alert): escape helm strval special chars in alert annotations  (Pierre Gerbelot)
## Release notes engine v1.209.1   
### Others   
* [8cb0c709ad04bd9a70f22008360633a9a717ea3d](https://github.com/Qovery/engine/commit/8cb0c709ad04bd9a70f22008360633a9a717ea3d): fix(QOV-1626): envoy access logs remove special chars  (benjaminch)
## Release notes engine v1.209.0   
### Features   
* [a60eb79dd3349d68cc9359706a45902d383fdc23](https://github.com/Qovery/engine/commit/a60eb79dd3349d68cc9359706a45902d383fdc23): feat(envoy): deploy experimental crds  (Σrebe - Romain GERARD)
   
* [5fd25499309e0d0ed6a93898a7edc2ee273c6c49](https://github.com/Qovery/engine/commit/5fd25499309e0d0ed6a93898a7edc2ee273c6c49): feat(helm): Allow new API gateway namespaced CRDs  (Σrebe - Romain GERARD)
   
### Bug fixes   
* [4ce94f1455a30a5a5342e7e5fbb83da04677d766](https://github.com/Qovery/engine/commit/4ce94f1455a30a5a5342e7e5fbb83da04677d766): fix(azure): add region-specific zones, fix attribute parsing, expose zones in API  (Guillaume Da Silva)
   
* [990ebfcaabd2f7458265e9b312ec233c3de6fe98](https://github.com/Qovery/engine/commit/990ebfcaabd2f7458265e9b312ec233c3de6fe98): fix: improve upgrade check error messages with specific reasons  (Guillaume Da Silva)
## Release notes engine v1.208.2   
### Bug fixes   
* [6c821d7987cc322560e7925ce995c51da6c82e6b](https://github.com/Qovery/engine/commit/6c821d7987cc322560e7925ce995c51da6c82e6b): fix(services): ndot config to be set in template only if defined  (benjaminch)
   
### Others   
* [36cd6a5ff79cdd0cbfbbc5baa452fe8f290d4446](https://github.com/Qovery/engine/commit/36cd6a5ff79cdd0cbfbbc5baa452fe8f290d4446): fix(QOV-1626): fix envoy access logs  (benjaminch)
## Release notes engine v1.208.1   
### Internal changes   
* [25d9e3ffdf5465050a344253f61a7fd96c8b0f57](https://github.com/Qovery/engine/commit/25d9e3ffdf5465050a344253f61a7fd96c8b0f57): chore(azure): make perm check not failling  (Romain GERARD)
   
### Others   
* [baae2822f337bfd727ea0ee46e7b4945e97800fd](https://github.com/Qovery/engine/commit/baae2822f337bfd727ea0ee46e7b4945e97800fd): refacto(qov-1564) Rename aws key for static credentials auth  (Melvin Zottola)
## Release notes engine v1.208.0   
### Bug fixes   
* [db045701aa41e394c7e666f1c40713d9d069478d](https://github.com/Qovery/engine/commit/db045701aa41e394c7e666f1c40713d9d069478d): fix: allow Let's Encrypt ACME challenges when IP whitelist is enabled  (Guillaume Da Silva)
   
### Others   
* [abb0a48fd024dc48c36fac8f10b37c4efc383944](https://github.com/Qovery/engine/commit/abb0a48fd024dc48c36fac8f10b37c4efc383944): feat(QOV-1613): add functionality to delete pods on stuck karpenter nodes to...  (Guillaume Dubroeucq)
## Release notes engine v1.207.0   
### Features   
* [c606e17f6d439f712ce8bd29e467ffffd706f57e](https://github.com/Qovery/engine/commit/c606e17f6d439f712ce8bd29e467ffffd706f57e): feat: support per-nodegroup zone for Scaleway Kapsule  (Guillaume Da Silva)
   
### Others   
* [689f29ccecd6d071ec0f3869ba3e54215e1698f0](https://github.com/Qovery/engine/commit/689f29ccecd6d071ec0f3869ba3e54215e1698f0): feat(qov-1564) Integrate External Secrets Operator  (Melvin Zottola)
## Release notes engine v1.206.1   
### Bug fixes   
* [207c42a863d60ee539a2c6966f6f40ba142dcf5f](https://github.com/Qovery/engine/commit/207c42a863d60ee539a2c6966f6f40ba142dcf5f): fix: propagate custom routes to private route tables in NAT gateway mode  (Guillaume Da Silva)
   
### Internal changes   
* [a26890dd22f470f7c912a9979df0d28b4f207066](https://github.com/Qovery/engine/commit/a26890dd22f470f7c912a9979df0d28b4f207066): chore(envoy): use new field for enabling proxy protocol  (Σrebe - Romain GERARD)
   
* [e19b8d92c79ca27f61bfae5366b97c12892c1d02](https://github.com/Qovery/engine/commit/e19b8d92c79ca27f61bfae5366b97c12892c1d02): chore(helper): Remove deprecated check  (Antoine Promerova)
## Release notes engine v1.206.0   
### Others   
* [9ca01f5f4246aee87f62101e5f11a7c74c4ecfb1](https://github.com/Qovery/engine/commit/9ca01f5f4246aee87f62101e5f11a7c74c4ecfb1): feat(QOV-1604): include entire repository in terraform packaging  (Fabien FLEUREAU)
## Release notes engine v1.205.3   
### Features   
* [815d19979013bf81a48cce425bf6493e37bc8644](https://github.com/Qovery/engine/commit/815d19979013bf81a48cce425bf6493e37bc8644): feat: support custom AMI for EKS clusters (Karpenter only)  (Guillaume Da Silva)
## Release notes engine v1.205.2   
### Others   
* [dc809fc51959229d1457d97a0cce5245a4146d66](https://github.com/Qovery/engine/commit/dc809fc51959229d1457d97a0cce5245a4146d66): fix(QOV-1606): gateway-api routes split when > 8 hotnames  (benjaminch)
## Release notes engine v1.205.1   
### Bug fixes   
* [628a85b6182185353c66246ac90bd6be464172f3](https://github.com/Qovery/engine/commit/628a85b6182185353c66246ac90bd6be464172f3): fix(bottlerocket): pin xvda to 4Gi OS disk, assign user disk size to xvdb  (Guillaume Da Silva)
## Release notes engine v1.205.0   
### Features   
* [be4eb583a3d1ca67ee23169dd1c0987c782a7167](https://github.com/Qovery/engine/commit/be4eb583a3d1ca67ee23169dd1c0987c782a7167): feat(terraform): Allow multiline variable in Jinja template  (Antoine)
## Release notes engine v1.204.1   
### Others   
* [3d5303ebadd865080285e0b85ea75072666c52d4](https://github.com/Qovery/engine/commit/3d5303ebadd865080285e0b85ea75072666c52d4): fix(scw-envoy): disable proxy v2  (benjaminch)
## Release notes engine v1.204.0   
### Bug fixes   
* [1d9e8efaf072abf6eab6ba545a789384f9071aec](https://github.com/Qovery/engine/commit/1d9e8efaf072abf6eab6ba545a789384f9071aec): fix(helm): throttle list_release and share cache across deploy levels  (Pierre Gerbelot)
   
### Others   
* [18e9bc6bbd091382d2054d3e7011453ad12d8049](https://github.com/Qovery/engine/commit/18e9bc6bbd091382d2054d3e7011453ad12d8049): feat(QOV-1595): new advanced setting not to deploy nginx  (benjaminch)
   
* [233ed84328e3b3d2d1cf4760a282346815dd1888](https://github.com/Qovery/engine/commit/233ed84328e3b3d2d1cf4760a282346815dd1888): revert: "feat(QOV-1595): new advanced setting not to deploy nginx"  (benjaminch)
## Release notes engine v1.203.2   
### Bug fixes   
* [d2fd8ae37d2385958823a8031a2c40cf12f7a20f](https://github.com/Qovery/engine/commit/d2fd8ae37d2385958823a8031a2c40cf12f7a20f):  fix(nginx): drop high-cardinality labels from nginx metrics on all providers  (Pierre Gerbelot)
## Release notes engine v1.203.1   
### Bug fixes   
* [8ab9ddf25af2532cb8473d20406ab6ba98911996](https://github.com/Qovery/engine/commit/8ab9ddf25af2532cb8473d20406ab6ba98911996): fix(karpenter): encrypt Bottlerocket second EBS volume (/dev/xvdb)  (Guillaume Da Silva)
   
* [3bea4f6d1d1872aba49c7e8ca67fbe75ef37d988](https://github.com/Qovery/engine/commit/3bea4f6d1d1872aba49c7e8ca67fbe75ef37d988): fix: delete settings.local.json  (benjaminch)
## Release notes engine v1.203.0   
### Others   
* [ec31ec5dd8ef02ccb39cc206ef902103220106b8](https://github.com/Qovery/engine/commit/ec31ec5dd8ef02ccb39cc206ef902103220106b8): feat(QOV-1595): new advanced setting not to deploy nginx  (benjaminch)
## Release notes engine v1.202.0   
### Features   
* [3b02c2cda5cd1409588af8fc114a1610ae5d5945](https://github.com/Qovery/engine/commit/3b02c2cda5cd1409588af8fc114a1610ae5d5945): feat(keda): move qovery_gateway_class_chart to a specific level for GCP  (Pierre Gerbelot)
## Release notes engine v1.201.0   
### Features   
* [bf4721af98869e0b4f92645ec03910d2a5d086fd](https://github.com/Qovery/engine/commit/bf4721af98869e0b4f92645ec03910d2a5d086fd): feat(keda): move keda to an other deployment level for GKE  (Pierre Gerbelot)
## Release notes engine v1.200.0   
### Bug fixes   
* [0ca02a5c9876fe0f2014521b4a87ed584a75c8ae](https://github.com/Qovery/engine/commit/0ca02a5c9876fe0f2014521b4a87ed584a75c8ae): fix(thanos): remove duplicated value  (Pierre Gerbelot)
   
### Others   
* [fc21c8b9f57b3c35247c323ac84da5998b3d2c8d](https://github.com/Qovery/engine/commit/fc21c8b9f57b3c35247c323ac84da5998b3d2c8d): feat(QOV-1597): add timeout advanced settings at services level  (benjaminch)
## Release notes engine v1.199.3   
### Bug fixes   
* [10c9e8a2dc0dcfaeaa780e8bed921ccbac0bb68a](https://github.com/Qovery/engine/commit/10c9e8a2dc0dcfaeaa780e8bed921ccbac0bb68a): fix: configure less agressive consolidation  (Guillaume Da Silva)
## Release notes engine v1.199.2   
### Bug fixes   
* [03660c7a571b306822b1ffed7678d29c4e717572](https://github.com/Qovery/engine/commit/03660c7a571b306822b1ffed7678d29c4e717572): fix(keda): remove hard-coded aws annotations  (Pierre Gerbelot)
## Release notes engine v1.199.1   
### Others   
* [38295f50fcb2e44605ecab16785369bdaba4fb62](https://github.com/Qovery/engine/commit/38295f50fcb2e44605ecab16785369bdaba4fb62): feat(QOV-1593): nginx to be spread across several nodes  (benjaminch)
## Release notes engine v1.199.0   
### Others   
* [38295f50fcb2e44605ecab16785369bdaba4fb62](https://github.com/Qovery/engine/commit/38295f50fcb2e44605ecab16785369bdaba4fb62): feat(QOV-1593): nginx to be spread across several nodes  (benjaminch)
## Release notes engine v1.198.1   
### Bug fixes   
* [00f6d24ecc5e639255a1cfcfd49ecb219b60cceb](https://github.com/Qovery/engine/commit/00f6d24ecc5e639255a1cfcfd49ecb219b60cceb): fix: use 0.40.1 image version for thanos to be able to connect to mexico region  (Pierre Gerbelot)
## Release notes engine v1.198.0   
### Features   
* [16227c8308412a7be815cd9b99f84cb1ac02388a](https://github.com/Qovery/engine/commit/16227c8308412a7be815cd9b99f84cb1ac02388a): feat(tags): propagate tag to nodes started by Karpenter  (Pierre Gerbelot)
## Release notes engine v1.197.0   
### Others   
* [59961a88c109ea1bc1ad862a42351181ae4edaf0](https://github.com/Qovery/engine/commit/59961a88c109ea1bc1ad862a42351181ae4edaf0): feat(QOV-1563): allow to customize dns ndots config in services  (benjaminch)
## Release notes engine v1.196.0   
### Others   
* [5b96620056cd891169eefe9e55084416f53a416c](https://github.com/Qovery/engine/commit/5b96620056cd891169eefe9e55084416f53a416c): feat(QOV-1583): configure Bottlerocket settings for karpenter nodegroup with max-pods  (Guillaume Dubroeucq)
## Release notes engine v1.195.0   
### Features   
* [6e17e5967c349165597665afae9d2a67f3affef0](https://github.com/Qovery/engine/commit/6e17e5967c349165597665afae9d2a67f3affef0): feat(EKS): add gp3 to karpenter nodegroup  (Guillaume Dubroeucq)
## Release notes engine v1.194.0   
### Others   
* [490e81689225f8592e4a8cbcf93da96d24ef1953](https://github.com/Qovery/engine/commit/490e81689225f8592e4a8cbcf93da96d24ef1953): Revert "feat(QOV-1583): update karpenter nodegroup max-pods and gp3"  (Guillaume Dubroeucq)
## Release notes engine v1.193.0   
### Features   
* [6545f5123f160ffe9b34dc63ee0e9484c82e250d](https://github.com/Qovery/engine/commit/6545f5123f160ffe9b34dc63ee0e9484c82e250d): feat(tags): allow to add custom tag to AWS resources  (Pierre Gerbelot)
   
### Others   
* [f8e05166ef3524f8ed2f23ee4f68d47485792faa](https://github.com/Qovery/engine/commit/f8e05166ef3524f8ed2f23ee4f68d47485792faa): feat(QOV-1525): add support for Pod Identity addon in EKS  (Guillaume Dubroeucq)
## Release notes engine v1.192.0   
### Others   
* [d8be7cb83807f4d7993c552d14100869cfff0390](https://github.com/Qovery/engine/commit/d8be7cb83807f4d7993c552d14100869cfff0390): feat(QOV-1583): update karpenter nodegroup max-pods and gp3  (Guillaume Dubroeucq)
## Release notes engine v1.191.0   
### Others   
* [a02edac8c590cf032d9414806462128a1c01d03b](https://github.com/Qovery/engine/commit/a02edac8c590cf032d9414806462128a1c01d03b): feat(QOV-1509): Scaleway to support Gateway API stack  (benjaminch)
   
* [b57acd38698d0dc63a22a7ff707276dcc9421862](https://github.com/Qovery/engine/commit/b57acd38698d0dc63a22a7ff707276dcc9421862): feat(gateway-api): Introduce proper structs for load-balancers  (benjaminch)
   
* [8b16d57ec4d2532fe0a21d3cc87b8594c1d37747](https://github.com/Qovery/engine/commit/8b16d57ec4d2532fe0a21d3cc87b8594c1d37747): fix(QOV-1556): envoy add HPA on gateways  (benjaminch)
   
* [0c7fbae81fc6b17a51760a108e48c1e5d6d1a663](https://github.com/Qovery/engine/commit/0c7fbae81fc6b17a51760a108e48c1e5d6d1a663): fix(QOV-1556): envoy add HPA on gateways  (benjaminch)
## Release notes engine v1.190.0   
### Bug fixes   
* [500420e9b748b719c834bb68a0c22ec05046f410](https://github.com/Qovery/engine/commit/500420e9b748b719c834bb68a0c22ec05046f410): fix(obs): remove secrets and configmaps from kube-state-metrics scraping  (Pierre Gerbelot)
   
### Others   
* [c0ca0a3f0edc58f3003c8e5d436ddb04832ea307](https://github.com/Qovery/engine/commit/c0ca0a3f0edc58f3003c8e5d436ddb04832ea307): feat(QOV-1556): envoy add HPA on gateways  (benjaminch)
## Release notes engine v1.189.1   
### Bug fixes   
* [a2b39dc672e30d3e0e61f96fc55cc9d330be8d68](https://github.com/Qovery/engine/commit/a2b39dc672e30d3e0e61f96fc55cc9d330be8d68): fix: force lowercase for Docker registry URLs  (Guillaume Da Silva)
## Release notes engine v1.189.0   
### Features   
* [3fd1830f7c8dad14b661998158941ad6c6cedc28](https://github.com/Qovery/engine/commit/3fd1830f7c8dad14b661998158941ad6c6cedc28): feat: add http metric for for envoy  (Pierre Gerbelot)
## Release notes engine v1.188.1   
### Bug fixes   
* [20767fe0c5d4ccda1595ca3e86483bb288ace4ee](https://github.com/Qovery/engine/commit/20767fe0c5d4ccda1595ca3e86483bb288ace4ee): fix(helm): remove secrets from helm error  (Σrebe - Romain GERARD)
## Release notes engine v1.188.0   
### Others   
* [ef6c00c8800e4ec23c293e8688671cd61877bb39](https://github.com/Qovery/engine/commit/ef6c00c8800e4ec23c293e8688671cd61877bb39): feat(QOV-1507): GCP to support Gateway API stack  (benjaminch)
   
* [747642296e29f4125643d17984e75fb056f0d6cc](https://github.com/Qovery/engine/commit/747642296e29f4125643d17984e75fb056f0d6cc): fix(secrets) Add raw json into secrets so it is obfuscated  (Antoine Promerova)
## Release notes engine v1.187.1   
### Others   
* [b80089be0bc5dbc01296898219329642ba57d566](https://github.com/Qovery/engine/commit/b80089be0bc5dbc01296898219329642ba57d566): fix(alert-config): provide namespace  (Pierre Gerbelot)
## Release notes engine v1.187.0   
### Features   
* [a6df23afa11bcfdeb292da73f9d7a340b50fe5cf](https://github.com/Qovery/engine/commit/a6df23afa11bcfdeb292da73f9d7a340b50fe5cf): feat: Add apply_immediately field for managed databases  (Guillaume Da Silva)
   
### Bug fixes   
* [4a92dc54b145403fc88a62c399bbfb31a721234c](https://github.com/Qovery/engine/commit/4a92dc54b145403fc88a62c399bbfb31a721234c): fix: deploy alert manager config for gcp and azure cluster  (Pierre Gerbelot)
   
### Internal changes   
* [61c92ebccc71fd52486a76b454abb0fb6a9a4ec7](https://github.com/Qovery/engine/commit/61c92ebccc71fd52486a76b454abb0fb6a9a4ec7): chore: remove deps  (Σrebe - Romain GERARD)
   
* [593b20ab6d5797404261a31773f0f75caf7a95a8](https://github.com/Qovery/engine/commit/593b20ab6d5797404261a31773f0f75caf7a95a8): chore: remove vault completly  (Σrebe - Romain GERARD)
   
### Tests   
* [2022c08fae57e6cd3a1f4e369ad29d342050cbd0](https://github.com/Qovery/engine/commit/2022c08fae57e6cd3a1f4e369ad29d342050cbd0): tests(azure): update test cluster kubeconfig secret  (benjaminch)
   
### Others   
* [3d32789203ef54b2b7f8719bc58fd1b59d645d6b](https://github.com/Qovery/engine/commit/3d32789203ef54b2b7f8719bc58fd1b59d645d6b): feat(QOV-997): implement helm chart deployment with automatic retry for transient errors  (Guillaume Dubroeucq)
## Release notes engine v1.186.0   
### Others   
* [2d79076902f580bccb87fb316523d310899db317](https://github.com/Qovery/engine/commit/2d79076902f580bccb87fb316523d310899db317): feat(QOV-1508): Azure to support Gateway API stack  (benjaminch)
## Release notes engine v1.185.0   
### Features   
* [a57fba83192059c43c684aba36952bdd30493f52](https://github.com/Qovery/engine/commit/a57fba83192059c43c684aba36952bdd30493f52): feat: improve AWS network resources naming with clear tags  (Guillaume Da Silva)
## Release notes engine v1.184.1   
### Bug fixes   
* [e9c4d6ff7727526907b90983edf5c1b8c9b72f41](https://github.com/Qovery/engine/commit/e9c4d6ff7727526907b90983edf5c1b8c9b72f41): fix(job): restore force_trigger for lifecycle jobs  (Fabien FLEUREAU)
## Release notes engine v1.184.0   
### Features   
* [b530cb274029e66b2b25e05604b6f50e4ec1ba16](https://github.com/Qovery/engine/commit/b530cb274029e66b2b25e05604b6f50e4ec1ba16): feat(keda): add GCP support  (Pierre Gerbelot)
## Release notes engine v1.183.0   
### Features   
* [a87ab9fcd9f171824cea11ad7bf2afadf0ab6d7b](https://github.com/Qovery/engine/commit/a87ab9fcd9f171824cea11ad7bf2afadf0ab6d7b): feat: add disk IOPS and throughput support for AWS EBS gp3 volumes  (Guillaume Da Silva)
## Release notes engine v1.182.1   
### Others   
* [841d2976bf106cf435f298a79fc6f9498117d06f](https://github.com/Qovery/engine/commit/841d2976bf106cf435f298a79fc6f9498117d06f): fix(QOV-1540): prevent Beyla DaemonSet from running on AWS nodegroups  (Pierre Gerbelot)
## Release notes engine v1.182.0   
### Features   
* [b402b3481043e3920e3d9bc03b55f56a4b177546](https://github.com/Qovery/engine/commit/b402b3481043e3920e3d9bc03b55f56a4b177546): feat(alert): improve email template for alertnotification  (Pierre Gerbelot)
   
### Others   
* [00cb613a025aa74906ad5ed0d2cdfe29c736b676](https://github.com/Qovery/engine/commit/00cb613a025aa74906ad5ed0d2cdfe29c736b676): chore(QOV-1400): allow to retrieve envoy gateway logs  (benjaminch)
## Release notes engine v1.181.0   
### Others   
* [e6b6b91cfadbc4aa265466752051622b42a25dea](https://github.com/Qovery/engine/commit/e6b6b91cfadbc4aa265466752051622b42a25dea): feat(QOV-1537): change karpenter priority class to system-node-critical  (Guillaume Dubroeucq)
## Release notes engine v1.180.1   
### Others   
* [45725ab28d3840c9cd5519945355eeeae70009c3](https://github.com/Qovery/engine/commit/45725ab28d3840c9cd5519945355eeeae70009c3): fix(QOV-1532): reduce CPU resource requests to 100m in karpenter.yaml  (Guillaume Dubroeucq)
   
* [8ed47584fa5461c557305383bc2084cc7ea4db90](https://github.com/Qovery/engine/commit/8ed47584fa5461c557305383bc2084cc7ea4db90): fix(QOV-1532): remove c6g.medium instance type from karpenter nodegroup  (Guillaume Dubroeucq)
## Release notes engine v1.180.0   
### Others   
* [3eb54afa966fe9783ff6bfe1eb7bfa3f3ec705a5](https://github.com/Qovery/engine/commit/3eb54afa966fe9783ff6bfe1eb7bfa3f3ec705a5): feat(QOV-1532): add instance type to karpenter nodegroup  (Guillaume Dubroeucq)
## Release notes engine v1.179.0   
### Features   
* [9a91c5274ff45c9f1644d108f4f6966706abc7e9](https://github.com/Qovery/engine/commit/9a91c5274ff45c9f1644d108f4f6966706abc7e9): feat(alert): add support for email receveir  (Pierre Gerbelot)
## Release notes engine v1.178.0   
### Features   
* [9a81d3c47057838da0218e98cdc06de4bd873bb5](https://github.com/Qovery/engine/commit/9a81d3c47057838da0218e98cdc06de4bd873bb5): feat(keda): allow 0 instance when keda is enabled  (Pierre Gerbelot)
   
### Internal changes   
* [f132e64a8205addd3d3a6b36fb63620a3c71fb3c](https://github.com/Qovery/engine/commit/f132e64a8205addd3d3a6b36fb63620a3c71fb3c): chore: improve payload parsing error  (benjaminch)
## Release notes engine v1.177.1   
### Bug fixes   
* [97d3511f2218cd3688b45004d13e14181b18ca39](https://github.com/Qovery/engine/commit/97d3511f2218cd3688b45004d13e14181b18ca39): fix: resolve terraform service deployment freeze deadlock (QOV-1444)  (Fabien FLEUREAU)
## Release notes engine v1.177.0   
### Features   
* [42f9b01a84645a82adf67748d2f9aa594492bd58](https://github.com/Qovery/engine/commit/42f9b01a84645a82adf67748d2f9aa594492bd58): feat: support terraform resource extraction from deployments [QOV-1444]  (Fabien FLEUREAU)
## Release notes engine v1.176.0   
### Features   
* [f907b99cb72e96e1f1036a40a8164b3e60c47fe8](https://github.com/Qovery/engine/commit/f907b99cb72e96e1f1036a40a8164b3e60c47fe8): feat(keda): add autoscaling support with secret reference transformation  (Pierre Gerbelot)
## Release notes engine v1.175.1   
### Others   
* [9b32d60d701f098a068fc10e7ebe412f47e6842b](https://github.com/Qovery/engine/commit/9b32d60d701f098a068fc10e7ebe412f47e6842b): chore(external-dns): prevent args source order from changing  (benjaminch)
## Release notes engine v1.175.0   
### Features   
* [e5c70819092f63aff3ccbf43702dc9cc5e0b052b](https://github.com/Qovery/engine/commit/e5c70819092f63aff3ccbf43702dc9cc5e0b052b): feat(keda): add fallback parameter in the ScaledObject  (Pierre Gerbelot)
## Release notes engine v1.174.0   
### Features   
* [0249bd7661c81bff8ab592918b7482de0c91c099](https://github.com/Qovery/engine/commit/0249bd7661c81bff8ab592918b7482de0c91c099): feat(keda): add keda profile and manage service monitor  (Pierre Gerbelot)
## Release notes engine v1.173.1   
### Bug fixes   
* [1861c8073c367ae075bd15555599bb11eec866d6](https://github.com/Qovery/engine/commit/1861c8073c367ae075bd15555599bb11eec866d6): fix(helm): shorten rollback timeout when unlocking a release  (Σrebe - Romain GERARD)
   
* [9f3e150f771c1516559ef0de932b912628179d57](https://github.com/Qovery/engine/commit/9f3e150f771c1516559ef0de932b912628179d57): fix(terraform): fix karpenter nodegroup lifecycle  (Guillaume Dubroeucq)
## Release notes engine v1.173.0   
### Others   
* [8d1f193c43d1cf18c1d0f0b3cd0e02ea969bce8b](https://github.com/Qovery/engine/commit/8d1f193c43d1cf18c1d0f0b3cd0e02ea969bce8b): chore(QOV-1400): add whole enchilada tests for EKS gateway API  (bchastanier)
   
* [ae52095234f89dc5dcc07cb2785a3981b8d6674d](https://github.com/Qovery/engine/commit/ae52095234f89dc5dcc07cb2785a3981b8d6674d): chore(QOV-1488): introduce cluster profile adv setting for coredns addon  (bchastanier)
   
* [424956bb94718569ebbc65f11573dd6e3f7339dc](https://github.com/Qovery/engine/commit/424956bb94718569ebbc65f11573dd6e3f7339dc): feat(QOV-1400): add cluster adv setting for compression  (bchastanier)
   
* [be4b20d36a3950b547d4105ee09d610e7771f105](https://github.com/Qovery/engine/commit/be4b20d36a3950b547d4105ee09d610e7771f105): feat(QOV-1400): add cluster adv setting for custom_http_errors  (bchastanier)
   
* [30dc55ed0d2572cc531002e69ab4e07f88780da5](https://github.com/Qovery/engine/commit/30dc55ed0d2572cc531002e69ab4e07f88780da5): feat(QOV-1400): add cluster adv setting for default backend  (bchastanier)
   
* [96fc194ab6f7ed44a1204f7ccf070eef9ace4413](https://github.com/Qovery/engine/commit/96fc194ab6f7ed44a1204f7ccf070eef9ace4413): feat(QOV-1400): add cluster adv setting for num of trusted hops  (bchastanier)
   
* [d1da712fafbd23adbbe298187c6eebf3d7994408](https://github.com/Qovery/engine/commit/d1da712fafbd23adbbe298187c6eebf3d7994408): feat(QOV-1400): add service adv setting for circuit breaker  (bchastanier)
   
* [b488498dd189dd94fd9764cf4522d083f259af64](https://github.com/Qovery/engine/commit/b488498dd189dd94fd9764cf4522d083f259af64): feat(QOV-1400): add service adv setting for custom_http_errors  (bchastanier)
   
* [b59dfeedfb5fe6957e57da10634dac5dca446fa4](https://github.com/Qovery/engine/commit/b59dfeedfb5fe6957e57da10634dac5dca446fa4): feat(QOV-1400): add service adv setting for envoy access log format  (bchastanier)
   
* [5627e6e0ba30d6135438939a385502efb939725a](https://github.com/Qovery/engine/commit/5627e6e0ba30d6135438939a385502efb939725a): feat(QOV-1400): fix envoy cluster adv settings  (bchastanier)
## Release notes engine v1.172.0   
### Features   
* [ad001179a62a9a3a18dcbbc5d183193cb3ed0fae](https://github.com/Qovery/engine/commit/ad001179a62a9a3a18dcbbc5d183193cb3ed0fae): feat(keda): allow cluster agent to retried with Keda ScaledOject  (Pierre Gerbelot)
## Release notes engine v1.171.2   
### Bug fixes   
* [cb8c620ac30bf052101a88913987820321221e02](https://github.com/Qovery/engine/commit/cb8c620ac30bf052101a88913987820321221e02): fix(keda): allow raw_yaml to define trigger-level fields in scalers  (Pierre Gerbelot)
   
### Internal changes   
* [f81127fea98b6ff4bc6c82b15f9e3e77658bca1c](https://github.com/Qovery/engine/commit/f81127fea98b6ff4bc6c82b15f9e3e77658bca1c): chore(ci): Use precedent changelog  (Σrebe - Romain GERARD)
## Release notes engine v1.171.1   
### Bug fixes   
* [7db9564b064939801b634f157660fc94cd24f28f](https://github.com/Qovery/engine/commit/7db9564b064939801b634f157660fc94cd24f28f): fix(keda): ensure TriggerAuthentication is created before ScaledObject  (Pierre Gerbelot)
## Changelog   
### Internal changes   
* 0f28943d5dc0beb3dd6ed14035ebf7798423ebf1 chore(ci): bump for changelog
## Changelog   
### Internal changes   
* 9ff33121f0ce8d902c89df55c557f624f57d0fcd chore(ci): update goreleaser
## Changelog   
### Internal changes   
* 67bed130c419cb1af5c2bf7a5e9bd2fe516eadd4 chore: reset changelog
