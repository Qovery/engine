resource "scaleway_k8s_cluster" "kubernetes_cluster"  {
  name    = var.kubernetes_cluster_name
  version = var.scaleway_ks_version
  cni     = "cilium"

  region  = var.region

  tags    = local.tags_ks_list

  autoscaler_config {
    # autoscaler FAQ https://github.com/kubernetes/autoscaler/blob/master/cluster-autoscaler/FAQ.md
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