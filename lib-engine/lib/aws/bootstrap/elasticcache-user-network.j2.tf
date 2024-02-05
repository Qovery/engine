{% set es_enabled = elasticache_subnets_zone_a_ids | default(value=[]) | length  %}
{%- if user_provided_network and es_enabled -%}

locals {
  tags_elasticache = merge(
    aws_eks_cluster.eks_cluster.tags,
    {
      "Service" = "Elasticache"
    }
  )
}



# Network is provided by the user
variable "elasticache_subnets_zone_a_ids" {
  type    = list(string)
  default = [
    {%- for id in elasticache_subnets_zone_a_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

variable "elasticache_subnets_zone_b_ids" {
  type    = list(string)
  default = [
    {%- for id in elasticache_subnets_zone_b_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

variable "elasticache_subnets_zone_c_ids" {
  type    = list(string)
  default = [
    {%- for id in elasticache_subnets_zone_c_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}


# Network

data "aws_subnet" "elasticache_zone_a" {
  count = length(var.elasticache_subnets_zone_a_ids)
  id    = var.elasticache_subnets_zone_a_ids[count.index]
}

data "aws_subnet" "elasticache_zone_b" {
  count = length(var.elasticache_subnets_zone_b_ids)
  id    = var.elasticache_subnets_zone_b_ids[count.index]
}

data "aws_subnet" "elasticache_zone_c" {
  count = length(var.elasticache_subnets_zone_c_ids)
  id    = var.elasticache_subnets_zone_c_ids[count.index]
}

#resource "aws_route_table_association" "elasticache_cluster_zone_a" {
#  count = length(var.elasticache_subnets_zone_a_ids)
#
#  subnet_id      = data.aws_subnet.elasticache_zone_a.*.id[count.index]
#  route_table_id = aws_route_table.eks_cluster.id
#}
#
#resource "aws_route_table_association" "elasticache_cluster_zone_b" {
#  count = length(var.elasticache_subnets_zone_b_ids)
#
#  subnet_id      = data.aws_subnet.elasticache_zone_b.*.id[count.index]
#  route_table_id = aws_route_table.eks_cluster.id
#}
#
#resource "aws_route_table_association" "elasticache_cluster_zone_c" {
#  count = length(var.elasticache_subnets_zone_c_ids)
#
#  subnet_id      = data.aws_subnet.elasticache_zone_c.*.id[count.index]
#  route_table_id = aws_route_table.eks_cluster.id
#}

resource "aws_elasticache_subnet_group" "elasticache" {
  description = "Elasticache linked to ${var.kubernetes_cluster_id}"
  # WARNING: this "name" value is used into elasticache clusters, you need to update it accordingly
  name = "elasticache-${data.aws_vpc.eks.id}"
  subnet_ids = flatten([data.aws_subnet.elasticache_zone_a.*.id, data.aws_subnet.elasticache_zone_b.*.id, data.aws_subnet.elasticache_zone_c.*.id])
}

{%- endif -%}
