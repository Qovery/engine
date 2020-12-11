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
# due to tags, (applied on kube cluster and pools), all droplets (scaled) will me automatically attached to the firewall below
resource "digitalocean_tag" "cluster_tag" {
  name = var.oks_master_name
}

resource "digitalocean_firewall" "qovery_firewall" {
  name = "k8s-${digitalocean_kubernetes_cluster.kubernetes_cluster.id}-worker"
  tags = [digitalocean_tag.cluster_tag.id]

// https://www.digitalocean.com/community/questions/using-do-managed-kubernetes-cluster-with-helm-chart-stable-prometheus-results-in-some-node_exporters-being-unreachable
  inbound_rule = [
    {
      protocol         = "tcp"
      port_range       = "9100"
      source_addresses = [var.cidr_block, "172.16.0.0/20", "192.168.0.0/16"]
    },
    {
      protocol         = "tcp"
      port_range       = "443"
      source_addresses = ["0.0.0.0/0"]
    },
  ]
}