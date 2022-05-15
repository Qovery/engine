resource "digitalocean_kubernetes_cluster" "kubernetes_cluster" {
  name = var.kubernetes_cluster_name
  region = var.region
  version = var.doks_version
  vpc_uuid = digitalocean_vpc.qovery_vpc.id

  # upgrade
  auto_upgrade = true
  surge_upgrade = true

  tags = concat(local.tags_ks_list, ["QoveryNodeGroupName:{{ doks_worker_nodes[0].name }}", "QoveryNodeGroupId:${var.kubernetes_cluster_id}-0"])

  node_pool {
    tags = concat(local.tags_ks_list, ["QoveryNodeGroupName:{{ doks_worker_nodes[0].name }}", "QoveryNodeGroupId:${var.kubernetes_cluster_id}-0"])
    name = var.kubernetes_cluster_id
    size = "{{ doks_worker_nodes[0].instance_type }}"
    # use Digital Ocean built-in cluster autoscaler
    auto_scale = true
    min_nodes  = "{{ doks_worker_nodes[0].min_nodes }}"
    max_nodes  = "{{ doks_worker_nodes[0].max_nodes }}"
  }
}