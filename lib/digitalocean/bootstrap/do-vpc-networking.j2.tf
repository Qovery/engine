resource "digitalocean_vpc" "qovery_vpc" {
  name     = var.vpc_name
  region   = var.region
  ip_range = var.cidr_block
}