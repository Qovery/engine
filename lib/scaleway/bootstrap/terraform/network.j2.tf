{% if create_private_network %}
  resource "scaleway_vpc_private_network" "private_network" {
  name = "private_network_${var.kubernetes_cluster_id}"
  tags = local.tags_ks_list
}
{% endif %}

{% if create_private_network and enable_public_gateway_nat %}
resource "scaleway_vpc_public_gateway_ip" "pgw_ip" {
  zone = var.zone
  tags = local.tags_ks_list
}

resource "scaleway_vpc_public_gateway" "pgw" {
  name            = "pgw_${var.kubernetes_cluster_id}"
  type            = var.public_gateway_type     # e.g. VPC-GW-S
  zone            = var.zone
  project_id      = var.scaleway_project_id
  ip_id           = scaleway_vpc_public_gateway_ip.pgw_ip.id
  bastion_enabled = false
  tags            = local.tags_ks_list
}

resource "scaleway_vpc_gateway_network" "pgw_pn" {
  gateway_id         = scaleway_vpc_public_gateway.pgw.id
  private_network_id = scaleway_vpc_private_network.private_network.id
  enable_masquerade  = true
  zone               = var.zone
  ipam_config {
    push_default_route = true
  }
}
{% endif %}