# On the first boot, it's required to remove the existing CNI to get them managed by helm
resource "null_resource" "delete_aws_managed_cni" {
  provisioner "local-exec" {
    command = <<EOT
kubectl -n kube-system delete daemonset aws-node;
kubectl -n kube-system delete clusterrole aws-node;
kubectl -n kube-system delete clusterrolebinding aws-node;
kubectl -n kube-system delete crd eniconfigs.crd.k8s.amazonaws.com;
kubectl -n kube-system delete serviceaccount aws-node
# sleep is to avoid: "rendered manifests contain a resource that already exists"
sleep 10
EOT
    environment = {
      KUBECONFIG = local_file.kubeconfig.filename
      AWS_ACCESS_KEY_ID = "{{ aws_access_key }}"
      AWS_SECRET_ACCESS_KEY = "{{ aws_secret_key }}"
      AWS_DEFAULT_REGION = "{{ aws_region }}"
    }
  }
}

resource "helm_release" "aws_vpc_cni" {
  name = "aws-vpc-cni"
  chart = "charts/aws-vpc-cni"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "image.tag"
    value = "v1.6.3"
  }

  set {
    name = "image.region"
    value = var.region
  }

  set {
    name = "image.pullPolicy"
    value = "IfNotPresent"
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

  depends_on = [
    aws_eks_cluster.eks_cluster,
    null_resource.delete_aws_managed_cni,
  ]
}
