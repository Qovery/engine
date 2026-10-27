# qovery-storage-class

This bundle installs the Qovery StorageClasses that Qovery services and container databases
request by name through the cluster advanced setting `storageclass.fast_ssd`. They reproduce the
legacy `q-storageclass-{aws,gcp,scaleway}` charts with the same names, provisioners and parameters.
It has no cluster-specific value and therefore no runtime input or evaluator.

The chart value `provider` selects the classes to render, and the provider overlay sets it:

| Overlay | Classes |
| --- | --- |
| `aws.yaml` | `aws-ebs-gp3-0`, `aws-ebs-gp2-0`, `aws-ebs-io1-0`, `aws-ebs-st1-0`, `aws-ebs-sc1-0` |
| `gcp.yaml` | `gcp-pd-extreme`, `gcp-pd-ssd`, `gcp-pd-balanced`, `gcp-pd-standard` |
| `scw.yaml` | `scw-sbv-ssd-0` |

Any other provider has no overlay and the release renders no class.

Differences with the legacy charts:

- no class is ever annotated as the cluster default: customer-managed clusters keep their own
  default StorageClass;
- the AWS `aws-efs` class and the GCP `disk-encryption-kms-key` parameter are not rendered,
  because the catalog has no input for an EFS file system or a KMS key.

The component belongs to the mandatory `qovery-stack` layer of `qovery-self-managed-v0` and has no
dependency. The provisioners must be available on the cluster: the EBS CSI driver on AWS (the
in-tree `kubernetes.io/aws-ebs` provisioner is migrated to it), the Compute Engine persistent disk
CSI driver on GKE, and the Scaleway Block Storage CSI driver on Kapsule.

Every class this release creates carries the annotation `qovery.com/source: platform-catalog`, so
`kubectl get storageclass -o yaml` tells them apart from classes created by the legacy bootstrap.

The release renders a class only when no class with that name exists yet, or when the existing
class carries the `app.kubernetes.io/managed-by: Helm` label and this release's
`meta.helm.sh/release-name` and `meta.helm.sh/release-namespace` annotations. An existing class owned by anything else, for example a class created by hand, by the
`kube-system/q-storageclass` release on a Qovery-managed cluster, or by the umbrella `qovery`
release on a cluster installed with the legacy BYOK chart, is left untouched and used as is: its
parameters are not checked and the release never manages it. `helm template` has no cluster to
look up, so it always renders every class of the provider.
StorageClass parameters are immutable: changing one requires deleting and recreating the class.

A class this release created, then deleted and recreated outside the release (for example to
change an immutable parameter or add a KMS key), is no longer rendered, so the next deployment
removes it: Helm deletes objects that left the manifest. The deployment after it recreates the
class with the Qovery parameters. To keep a customized class, recreate it under a different name
and point the service storage setting at it. Keeping the release label and annotations on it does
not help: the release then manages it again and patches its parameters back to the Qovery values,
which the API server rejects because they are immutable.
