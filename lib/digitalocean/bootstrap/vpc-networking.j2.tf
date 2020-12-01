resource "digitalocean_vpc" "qovery_vpc" {
  name     = var.vpc_name
  region   = var.digitalocean_region
  ip_range = var.cidr_block
}