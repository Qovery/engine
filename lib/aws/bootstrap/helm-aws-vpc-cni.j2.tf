locals {
  aws_cni_chart_release_name = "aws-vpc-cni"
}

# On the first boot, it's required to remove the existing CNI to get them managed by helm
resource "null_resource" "enable_cni_managed_by_helm" {
  provisioner "local-exec" {
    command = <<EOT
if [ "$(kubectl -n kube-system get daemonset -l k8s-app=aws-node,app.kubernetes.io/managed-by=Helm 2>&1 | grep -ic 'No resources found')" == "0" ] ; then
  exit 0
fi

for kind in daemonSet clusterRole clusterRoleBinding serviceAccount; do
  echo "setting annotations and labels on $kind/aws-node"
  kubectl -n kube-system annotate --overwrite $kind aws-node meta.helm.sh/release-name=${local.aws_cni_chart_release_name}
  kubectl -n kube-system annotate --overwrite $kind aws-node meta.helm.sh/release-namespace=kube-system
  kubectl -n kube-system label --overwrite $kind aws-node app.kubernetes.io/managed-by=Helm
done
EOT

    environment = {
      KUBECONFIG = local_file.kubeconfig.filename
      AWS_ACCESS_KEY_ID = "{{ aws_access_key }}"
      AWS_SECRET_ACCESS_KEY = "{{ aws_secret_key }}"
      AWS_DEFAULT_REGION = "{{ aws_region }}"
    }
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
  ]
}

resource "helm_release" "aws_vpc_cni" {
  name = local.aws_cni_chart_release_name
  chart = "charts/aws-vpc-cni"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  set {
    name = "image.region"
    value = var.region
  }

  set {
    name = "image.pullPolicy"
    value = "IfNotPresent"
  }

  set {
    name = "crd.create"
    value = "false"
  }

  # label ENIs
  set {
    name = "env.CLUSTER_NAME"
    value = var.kubernetes_cluster_name
  }

  ## POD ALLOCATION ##
  # number of total IP addresses that the daemon should attempt to allocate for pod assignment on the node (init phase)
  set {
    name = "env.MINIMUM_IP_TARGET"
    value = "60"
  }

  # number of free IP addresses that the daemon should attempt to keep available for pod assignment on the node
  set {
    name = "env.WARM_IP_TARGET"
    value = "10"
  }

  # maximum number of ENIs that will be attached to the node (k8s recommend to avoid going over 100)
  set {
    name = "env.MAX_ENI"
    value = "100"
  }

  # Limits
  set {
    name = "resources.limits.cpu"
    value = "200m"
  }

  set {
    name = "resources.requests.cpu"
    value = "50m"
  }

  set {
    name = "resources.limits.memory"
    value = "128Mi"
  }

  set {
    name = "resources.requests.memory"
    value = "128Mi"
  }

  set {
    name = "forced_upgrade"
    value = var.forced_upgrade
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    null_resource.enable_cni_managed_by_helm,
    {% if not test_cluster %}
    vault_generic_secret.cluster-access,
    {% endif %}
  ]
}