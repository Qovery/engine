{% for scw_ks_worker_node in scw_ks_worker_nodes %}
resource "scaleway_k8s_pool" "kubernetes_cluster_workers_{{ loop.index }}" {
  cluster_id    = scaleway_k8s_cluster.kubernetes_cluster.id
  name          = "${var.kubernetes_cluster_id}_{{ scw_ks_worker_node.instance_type }}_{{ loop.index }}"
  node_type     = "{{ scw_ks_worker_node.instance_type }}"

  region        = var.region
  zone          = var.zone

  # use Scaleway built-in cluster autoscaler
  autoscaling         = {{ scw_ks_pool_autoscale }}
  autohealing         = true
  size                = "{{ scw_ks_worker_node.min_nodes }}"
  min_size            = "{{ scw_ks_worker_node.min_nodes }}"
  max_size            = "{{ scw_ks_worker_node.max_nodes }}"
  wait_for_pool_ready = true

  timeouts {
    create = "60m"
    update = "60m"
  }

  depends_on    = [
    scaleway_k8s_cluster.kubernetes_cluster,
  ]

  lifecycle {
    create_before_destroy = true
  }
  tags          =  concat(local.tags_ks_list, ["QoveryNodeGroupName:{{ scw_ks_worker_node.name }}", "QoveryNodeGroupId:${var.kubernetes_cluster_id}_{{ scw_ks_worker_node.instance_type }}_{{ loop.index }}"])
}
{% endfor %}
