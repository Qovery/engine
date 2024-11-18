{% if create_private_network %}
resource "scaleway_vpc_private_network" "private_network" {
  name = "private_network_${var.kubernetes_cluster_id}"
  tags = local.tags_ks_list
}
{% endif %}
resource "scaleway_k8s_cluster" "kubernetes_cluster"  {
  name    = var.kubernetes_cluster_name
  version = var.scaleway_ks_version
  cni     = "cilium"
  delete_additional_resources = true

  region  = var.region

  tags    = local.tags_ks_list
  {% if create_private_network %}
  private_network_id = scaleway_vpc_private_network.private_network.id
  {% endif %}

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

  timeouts {
    create = "30m"
    update = "60m"
  }
}
