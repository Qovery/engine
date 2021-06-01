locals {
  aws_cni_chart_release_name = "aws-vpc-cni"
}

data "external" "is_cni_old_installed_version" {
  program = ["./helper.sh", "is_cni_old_installed_version"]
  depends_on = [
    aws_eks_cluster.eks_cluster,
    null_resource.enable_cni_managed_by_helm,
  ]
}

# On the first boot, it's required to remove the existing CNI to get them managed by helm
resource "null_resource" "enable_cni_managed_by_helm" {
  provisioner "local-exec" {
    command = <<EOT
./helper.sh enable_cni_managed_by_helm
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

locals {
  aws_cni = <<CNI
crd:
  create: false
CNI
}

resource "helm_release" "aws_vpc_cni" {
  name = local.aws_cni_chart_release_name
  chart = "charts/aws-vpc-cni"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  values = [
    local.aws_cni,
  ]

  set {
    name = "image.region"
    value = var.region
    type = "string"
  }

  set {
    name = "image.pullPolicy"
    value = "IfNotPresent"
    type = "string"
  }

  set {
    name = "originalMatchLabels"
    value = data.external.is_cni_old_installed_version.result.is_cni_old_installed_version
    type = "string"
  }

  # label ENIs
  set {
    name = "env.CLUSTER_NAME"
    value = var.kubernetes_cluster_name
    type = "string"
  }

  ## POD ALLOCATION ##
  # number of total IP addresses that the daemon should attempt to allocate for pod assignment on the node (init phase)
  set {
    name = "env.MINIMUM_IP_TARGET"
    value = "60"
    type = "string"
  }

  # number of free IP addresses that the daemon should attempt to keep available for pod assignment on the node
  set {
    name = "env.WARM_IP_TARGET"
    value = "10"
    type = "string"
  }

  # maximum number of ENIs that will be attached to the node (k8s recommend to avoid going over 100)
  set {
    name = "env.MAX_ENI"
    value = "100"
    type = "string"
  }

  # Limits
  set {
    name = "resources.requests.cpu"
    value = "50m"
    type = "string"
  }

  set {
    name = "forced_upgrade"
    value = var.forced_upgrade
    type = "string"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    null_resource.enable_cni_managed_by_helm,
    data.external.is_cni_old_installed_version,
    {% if not test_cluster %}
    vault_generic_secret.cluster-access,
    {% endif %}
  ]
}
