{%- if user_provided_network -%}

locals {
  tags_elasticsearch = merge(
    local.tags_eks,
    {
      "Service" = "Elasticsearch"
    }
  )
}

# Network is provided by the user
variable "elasticsearch_subnets_zone_a_ids" {
  type    = list(string)
  default = [
    {%- for id in elasticsearch_subnets_zone_a_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

variable "elasticsearch_subnets_zone_b_ids" {
  type    = list(string)
  default = [
    {%- for id in elasticsearch_subnets_zone_b_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

variable "elasticsearch_subnets_zone_c_ids" {
  type    = list(string)
  default = [
    {%- for id in elasticsearch_subnets_zone_c_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

# Network
data "aws_subnet" "elasticsearch_zone_a" {
  count = length(var.elasticsearch_subnets_zone_a_ids)
  id    = var.elasticsearch_subnets_zone_a_ids[count.index]
}

data "aws_subnet" "elasticsearch_zone_b" {
  count = length(var.elasticsearch_subnets_zone_b_ids)
  id    = var.elasticsearch_subnets_zone_b_ids[count.index]
}

data "aws_subnet" "elasticsearch_zone_c" {
  count = length(var.elasticsearch_subnets_zone_c_ids)
  id    = var.elasticsearch_subnets_zone_c_ids[count.index]
}

resource "aws_route_table_association" "elasticsearch_cluster_zone_a" {
  count = length(var.elasticsearch_subnets_zone_a_ids)

  subnet_id      = data.aws_subnet.elasticsearch_zone_a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "elasticsearch_cluster_zone_b" {
  count = length(var.elasticsearch_subnets_zone_b_ids)

  subnet_id      = data.aws_subnet.elasticsearch_zone_b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "elasticsearch_cluster_zone_c" {
  count = length(var.elasticsearch_subnets_zone_c_ids)

  subnet_id      = data.aws_subnet.elasticsearch_zone_c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_security_group" "elasticsearch" {
  name = "elasticsearch-${var.kubernetes_cluster_id}"
  description = "Elasticsearch security group"
  vpc_id = data.aws_vpc.eks.id

  ingress {
    from_port = 443
    to_port = 443
    protocol = "tcp"

    cidr_blocks = [
      data.aws_vpc.eks.cidr_block
    ]
  }

  tags = local.tags_elasticsearch
}

{%- endif -%}
