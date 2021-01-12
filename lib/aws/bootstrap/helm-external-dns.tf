resource "helm_release" "externaldns" {
  name = "externaldns"
  chart = "common/charts/external-dns"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  values = [file("chart_values/external-dns.yaml")]

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "resources.limits.cpu"
    value = "50m"
  }

  set {
    name = "resources.requests.cpu"
    value = "50m"
  }

  set {
    name = "resources.limits.memory"
    value = "50Mi"
  }

  set {
    name = "resources.requests.memory"
    value = "50Mi"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.cluster_autoscaler,
    helm_release.aws_vpc_cni,
  ]
}