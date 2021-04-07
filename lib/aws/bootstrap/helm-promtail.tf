resource "helm_release" "promtail" {
  name = "promtail"
  chart = "common/charts/promtail"
  namespace = "logging"
  create_namespace = true
  atomic = true
  max_history = 50

  set {
    name = "loki.serviceName"
    value = "loki"
  }

  # it's mandatory to get this class to ensure paused infra will behave properly on restore
  set {
    name = "priorityClassName"
    value = "system-node-critical"
  }

  # Limits
  set {
    name = "resources.limits.cpu"
    value = "100m"
  }

  set {
    name = "resources.requests.cpu"
    value = "100m"
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
    helm_release.aws_vpc_cni,
  ]
}
