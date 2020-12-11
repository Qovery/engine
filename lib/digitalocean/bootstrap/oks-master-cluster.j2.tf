locals {
  tags_oks = {
    ClusterId = var.oks_cluster_id,
    ClusterName = var.oks_master_name,
    Region = var.region
  }
}

resource "digitalocean_kubernetes_cluster" "kubernetes_cluster" {
  name = var.oks_master_name
  region = var.region
  version = var.oks_version
  vpc_uuid = digitalocean_vpc.qovery_vpc.id

  node_pool {
    tags = [digitalocean_tag.cluster_tag.id]
    name = var.oks_master_name
    size = var.oks_master_size
    auto_scale = var.oks_master_autoscale
    node_count = var.oks_master_node_count
  }
}
