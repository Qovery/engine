resource "helm_release" "pleco" {
  count = var.test_cluster == "false" ? 0 : 1

  name = "pleco"
  chart = "common/charts/pleco"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  set {
    name = "enabledFeatures.disableDryRun"
    value = "true"
  }

  set {
    name = "environmentVariables.LOG_LEVEL"
    value = "debug"
  }

  set {
    name = "forced_upgrade"
    value = var.forced_upgrade
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster
  ]
}