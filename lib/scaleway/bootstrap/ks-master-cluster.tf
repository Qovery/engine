resource "scaleway_k8s_cluster" "kubernetes_cluster" {
  name    = "test-cluster" # TODO(benjaminch) : use : "qovery-${var.kubernetes_cluster_id}"
  version = "1.20.1" # TODO(benjaminch): Scaleway doesn't support omitting update version :(
  cni     = "cilium"

  tags    = local.tags_ks

  autoscaler_config {
    max_graceful_termination_sec      = 3600

    # disable_scale_down              = false (Default)
    # estimator                       = "binpacking" (Default)
    # scale_down_delay_after_add      = "10m" (Default)
  }

  auto_upgrade {
    enable = true
    maintenance_window_day = "any"
    maintenance_window_start_hour = 3
  }
}