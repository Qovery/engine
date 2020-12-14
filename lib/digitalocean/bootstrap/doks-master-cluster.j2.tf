resource "digitalocean_kubernetes_cluster" "kubernetes_cluster" {
  name = var.doks_master_name
  region = var.region
  version = var.doks_version
  vpc_uuid = digitalocean_vpc.qovery_vpc.id

  node_pool {
    tags = [digitalocean_tag.cluster_tag.id]
    name = var.doks_master_name
  {% for doks_worker_node in doks_worker_nodes %}
  {%- if loop.index == 1  %}
    size = "{{ doks_worker_node.instance_type }}"
    auto_scale = true
    min_nodes  = "{{ doks_worker_node.min_size }}"
    max_nodes  = "{{ doks_worker_node.max_size }}"
{%- endif %}
{% endfor %}
  }
}
