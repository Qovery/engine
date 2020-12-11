resource "helm_release" "pleco" {
  count = var.test_cluster == "false" ? 0 : 1

  name = "pleco"
  chart = "common/charts/pleco"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "enabledFeatures.disableDryRun"
    value = "true"
  }

  set {
    name = "environmentVariables.LOG_LEVEL"
    value = "debug"
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster
  ]
}