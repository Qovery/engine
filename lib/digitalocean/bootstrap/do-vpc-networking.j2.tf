resource "digitalocean_vpc" "qovery_vpc" {
  name     = var.vpc_name
  region   = var.region

{%- if 'manual' == do_vpc_cidr_set %}
  # Note: if set to manual, then it means a CIDR has been specified and needs to be declared.
  # The range of IP addresses for the VPC in CIDR notation.
  # Network ranges cannot overlap with other networks in the same account and must be in range of private addresses as defined in RFC1918.
  # It may not be larger than /16 or smaller than /24
  ip_range = var.cidr_block
{%- endif %}
}