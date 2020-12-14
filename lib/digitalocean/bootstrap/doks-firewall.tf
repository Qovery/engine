# due to tags, (applied on kube cluster and pools), all droplets (scaled) will me automatically attached to the firewall below
resource "digitalocean_tag" "cluster_tag" {
  name = var.doks_cluster_id
}

resource "digitalocean_firewall" "qovery_firewall" {
  name = "k8s-${digitalocean_kubernetes_cluster.kubernetes_cluster.id}-qovery-additional-fw"
  tags = [digitalocean_tag.cluster_tag.id]

  // https://www.digitalocean.com/community/questions/using-do-managed-kubernetes-cluster-with-helm-chart-stable-prometheus-results-in-some-node_exporters-being-unreachable
  inbound_rule {
    protocol         = "tcp"
    port_range       = "9100"
    source_addresses = [var.cidr_block, "172.16.0.0/20", "192.168.0.0/16"]
  }

  inbound_rule {
    protocol         = "tcp"
    port_range       = "443"
    source_addresses = ["0.0.0.0/0"]
  }
}