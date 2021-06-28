resource "scaleway_k8s_cluster" "kubernetes_cluster" {
  name    = "test-cluster" # TODO(benjaminch) : use : "qovery-${var.kubernetes_cluster_id}"
  version = "1.20" # TODO(benjaminch): Scaleway doesn't support omitting update version :(
  cni     = "cilium"

  tags    =  [for i, v in local.tags_ks : "${i}=${v}"] # NOTE: Scaleway doesn't support KV style tags

  autoscaler_config {
    max_graceful_termination_sec    = 3600
    disable_scale_down              = false
    estimator                       = "binpacking"
    scale_down_delay_after_add      = "10m"
    balance_similar_node_groups     = true
  }

  auto_upgrade {
    enable = true
    maintenance_window_day = "any"
    maintenance_window_start_hour = 3
  }
}