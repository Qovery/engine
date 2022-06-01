# Helm chart

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
