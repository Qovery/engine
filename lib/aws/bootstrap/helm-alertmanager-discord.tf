resource "helm_release" "alertmanager_discord" {
  name = "alertmanager-discord"
  chart = "common/charts/alertmanager-discord"
  namespace = "prometheus"
  create_namespace = true
  atomic = true
  max_history = 50

  set {
    name = "replicaCount"
    value = "1"
  }

  # Interrupt channel
  set {
    name = "application.environmentVariables.DISCORD_WEBHOOK"
    value = var.discord_api_key
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

  set {
    name = "forced_upgrade"
    value = var.forced_upgrade
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.aws_vpc_cni,
    helm_release.cluster_autoscaler,
  ]
}