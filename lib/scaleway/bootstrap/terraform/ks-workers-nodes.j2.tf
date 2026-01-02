{% for scw_ks_worker_node in scw_ks_worker_nodes %}
resource "scaleway_k8s_pool" "kubernetes_cluster_workers_{{ loop.index }}" {
  cluster_id    = scaleway_k8s_cluster.kubernetes_cluster.id
  node_type     = "{{ scw_ks_worker_node.instance_type }}"

  {% if create_private_network and enable_public_gateway_nat %}
  # when activating static IP, the node group will be recreated, hence we need a unique name
  # name to include instance type and disk size, because such changes requires creating a new pool and name should be unique
  name          = "${var.kubernetes_cluster_id}_{{ scw_ks_worker_node.instance_type }}_{{ scw_ks_worker_node.disk_size_in_gib }}_with_public_gateway_{{ loop.index }}"
  {%else %}
  # name to include instance type and disk size, because such changes requires creating a new pool and name should be unique
  name          = "${var.kubernetes_cluster_id}_{{ scw_ks_worker_node.instance_type }}_{{ scw_ks_worker_node.disk_size_in_gib }}_{{ loop.index }}"
  {% endif %}

  region        = var.region
  zone          = var.zone

  # use Scaleway built-in cluster autoscaler
  autoscaling         = {{ scw_ks_pool_autoscale }}
  autohealing         = true
  size                = "{{ scw_ks_worker_node.min_nodes }}"
  min_size            = "{{ scw_ks_worker_node.min_nodes }}"
  max_size            = "{{ scw_ks_worker_node.max_nodes }}"
  wait_for_pool_ready = false

  root_volume_size_in_gb = {{ scw_ks_worker_node.disk_size_in_gib }}

{% if create_private_network and enable_public_gateway_nat %}
  public_ip_disabled = true
{% endif %}

  timeouts {
    create = "30m"
    update = "60m"
  }

  lifecycle {
    create_before_destroy = true
  }
  tags          =  concat(local.tags_ks_list, ["QoveryNodeGroupName:{{ scw_ks_worker_node.name }}", "QoveryNodeGroupId:${var.kubernetes_cluster_id}_{{ scw_ks_worker_node.instance_type }}_{{ loop.index }}"])
}
{% endfor %}
