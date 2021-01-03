# This resource block is useful to add another worker to the cluster
# The first worker node is create into digitalocean_kubernetes_cluster terraform resource
{%- if doks_worker_nodes|length > 1 %}
{% for doks_worker_node in doks_worker_nodes %}
{%- if loop.index > 1 %}
resource "digitalocean_kubernetes_node_pool" "app_node_pool_{{ loop.index }}" {
  cluster_id = digitalocean_kubernetes_cluster.kubernetes_cluster.id

  name = "qovery-{{kubernetes_cluster_id}}-{{ loop.index }}"
  size = "{{ doks_worker_node.instance_type }}"
  tags = [digitalocean_tag.cluster_tag.id]
  auto_scale = true
  min_nodes  = "{{ doks_worker_node.min_size }}"
  max_nodes  = "{{ doks_worker_node.max_size }}"

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster,
  ]
}
{%- endif %}
{% endfor %}
{%- endif %}