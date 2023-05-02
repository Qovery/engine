# Helm chart

## v2.17.2
* Bump driver version to `v1.17.0`
* Bump `external-resizer` version to `v4.2.0`
* All other sidecars have been updated to the latest rebuild (without an associated version change)

## v2.17.1
* Bump driver version to `v1.16.1`

## v2.17.0
* Bump driver version to `v1.16.0`
* Add support for JSON logging ([#1467](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1467), [@torredil](https://github.com/torredil))
    * `--logging-format` flag has been added to set the log format. Valid values are `text` and `json`. The default value is `text`.
    * `--logtostderr` is deprecated.
    * Long arguments prefixed with `-` are no longer supported, and must be prefixed with `--`. For example, `--volume-attach-limit` instead of `-volume-attach-limit`.
* The sidecars have been updated. The new versions are:
    - csi-provisioner: `v3.4.0`
    - csi-attacher: `v4.1.0`
    - csi-snapshotter: `v6.2.1`
    - livenessprobe: `v2.9.0`
    - csi-resizer: `v1.7.0`
    - node-driver-registrar: `v2.7.0`


## v2.16.0
* Bump driver version to `v1.15.0`
* Change default sidecars to EKS-D ([#1475](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1475), [@ConnorJC3](https://github.com/ConnorJC3), [@torredil](https://github.com/torredil))
* The sidecars have been updated. The new versions are:
    - csi-provisioner: `v3.3.0`
    - csi-attacher: `v4.0.0`
    - csi-snapshotter: `v6.1.0`
    - livenessprobe: `v2.8.0`
    - csi-resizer: `v1.6.0`
    - node-driver-registrar: `v2.6.2`

## v2.15.1
* Bugfix: Prevent deployment of testing resources during normal installation by adding `helm.sh/hook: test` annotation.

## v2.15.0
* Set sensible default resource requests/limits
* Add sensible default update strategy
* Add podAntiAffinity so controller pods prefer scheduling on separate nodes if possible
* Add container registry parameter

## v2.14.2
* Bump driver version to `v1.14.1`

## v2.14.1
* Add `controller.sdkDebugLog` parameter

## v2.14.0
* Bump driver version to `v1.14.0`

## v2.13.0
* Bump app/driver to version `v1.13.0`
* Expose volumes and volumeMounts for the ebs-csi-controller deployment ([#1400](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1436), [@cnmcavoy](https://github.com/cnmcavoy))
* refactor: Move the default controller tolerations in the helm chart values ([#1427](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1427), [@cnmcavoy](https://github.com/Linutux42))
* Add serviceMonitor.labels parameter ([#1419](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1419), [@torredil](https://github.com/torredil))
* Add parameter to force enable snapshotter sidecar ([#1418](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1418), [@ConnorJC3](https://github.com/ConnorJC3))

## v2.12.1
* Bump app/driver to version `v1.12.1`

## v2.12.0
* Bump app/driver to version `v1.12.0`
* Move default toleration to values.yaml so it can be overriden if desired by users ([#1400](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1400), [@cnmcavoy](https://github.com/cnmcavoy))
* Add enableMetrics configuration ([#1380](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1380), [@torredil](https://github.com/torredil))
* add initContainer to the controller's template ([#1379](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1379), [@InsomniaCoder](https://github.com/InsomniaCoder))
* Add controller nodeAffinity to prefer EC2 over Fargate ([#1360](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1360), [@torredil](https://github.com/torredil))

## v2.11.1
* Add `useOldCSIDriver` parameter to use old `CSIDriver` object.

## v2.11.0

**Important Notice:** This version updates the `CSIDriver` object in order to fix [a bug with static volumes and the `fsGroup` parameter](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/issues/1365). This upgrade will fail on existing clusters because the associated field in `CSIDriver` is immutable.

Users upgrading to this version should pre-delete the existing `CSIDriver` object (example: `kubectl delete csidriver ebs.csi.aws.com`). This will not affect any existing volumes, but will cause the EBS CSI Driver to be unavailable to handle future requests, and should be immediately followed by an upgrade. For users that cannot delete the `CSIDriver` object, v2.11.1 implements a new parameter `useOldCSIDriver` that will use the previous `CSIDriver`.

* Bump app/driver to version `v1.11.3`
* Add support for leader election tuning for `csi-provisioner` and `csi-attacher` ([#1371](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1371), [@moogzy](https://github.com/moogzy))
* Change `fsGroupPolicy` to `File` ([#1377](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1377), [@ConnorJC3](https://github.com/ConnorJC3))
* Allow all taint for `csi-node` by default ([#1381](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1381), [@gtxu](https://github.com/gtxu))

## v2.10.1
* Bump app/driver to version `v1.11.2`

## v2.10.0
* Implement securityContext for containers
* Add securityContext for node pod
* Utilize more secure defaults for securityContext

## v2.9.0
* Bump app/driver to version `v1.10.0`
* Feature: Reference `configMaps` across multiple resources using `envFrom` ([#1312](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1312), [@jebbens](https://github.com/jebbens))

## v2.8.1
* Bump app/driver to version `v1.9.0`
* Update livenessprobe to version `v2.6.0`

## v2.8.0
* Feature: Support custom affinity definition on node daemon set ([#1277](https://github.com/kubernetes-sigs/aws-ebs-csi-driver/pull/1277), [@vauchok](https://github.com/vauchok))

## v2.7.1
* Bump app/driver to version `v1.8.0`

## v2.7.0
* Support optional ec2 endpoint configuration.
* Fix node driver registrar socket path.
* Fix hardcoded kubelet path.

## v2.6.11
* Bump app/driver to version `v1.7.0`
* Set handle-volume-inuse-error to `false`

## v2.6.10

* Add quotes around the `extra-tags` argument in order to prevent special characters such as `":"` from breaking the manifest YAML after template rendering.

## v2.6.9

* Update csi-snapshotter to version `v6.0.1`
* Update external-attacher to version `v3.4.0`
* Update external-resizer to version `v1.4.0`
* Update external-provisioner to version `v3.1.0`
* Update node-driver-registrar to version `v2.5.1`
* Update livenessprobe to version `v2.5.0`

## v2.6.8

* Bump app/driver to version `v1.6.2`
* Bump sidecar version for nodeDriverRegistrar, provisioner to be consistent with EKS CSI Driver Add-on

## v2.6.7

* Bump app/driver to version `v1.6.1`

## v2.6.6

* Bump app/driver to version `v1.6.0`

## v2.6.5

* Bump app/driver to version `v1.5.3`

## v2.6.4

* Remove exposure all secrets to external-snapshotter-role

## v2.6.3

* Bump app/driver to version `v1.5.1`

## v2.6.2

* Update csi-resizer version to v1.1.0

## v2.6.1

* Add securityContext support for controller Deployment

## v2.5.0

* Bump app/driver version to `v1.5.0`

## v2.4.1

* Replace deprecated arg `--extra-volume-tags` by `--extra-tags`

## v2.4.0

* Bump app/driver version to `v1.4.0`

## v2.3.1

* Bump app/driver version to `v1.3.1`

## v2.3.0

* Support overriding controller `--default-fstype` flag via values

## v2.2.1

* Bump app/driver version to `v1.3.0`

## v2.2.0

* Support setting imagePullPolicy for all containers

## v2.1.1

* Bump app/driver version to `v1.2.1`

## v2.1.0

* Custom `controller.updateStrategy` to set controller deployment strategy.

## v2.0.4

* Use chart app version as default image tag
* Add updateStrategy to daemonsets

## v2.0.3

* Bump app/driver version to `v1.2.0`

## v2.0.2

* Bump app/driver version to `v1.1.3`

## v2.0.1

* Only create Windows daemonset if enableWindows is true
* Update Windows daemonset to align better to the Linux one

## v2.0.0

* Remove support for Helm 2
* Remove deprecated values
* No longer install snapshot controller or its CRDs
* Reorganize additional values

[Upgrade instructions](/docs/README.md#upgrading-from-version-1x-to-2x-of-the-helm-chart)

## v1.2.4

* Bump app/driver version to `v1.1.1`
* Install VolumeSnapshotClass, VolumeSnapshotContent, VolumeSnapshot CRDs if enableVolumeSnapshot is true
* Only run csi-snapshotter sidecar if enableVolumeSnapshot is true or if CRDs are already installed
