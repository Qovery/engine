{%- if not user_provided_network -%}

locals {
  tags_elasticsearch = merge(
    local.tags_eks,
    {
      "Service" = "Elasticsearch"
    }
  )
}

# Elasticsearch
variable "elasticsearch_subnets_zone_a" {
  description = "Elasticsearch subnets Zone A"
  default = {{ elasticsearch_zone_a_subnet_blocks }}
  type = list(string)
}

variable "elasticsearch_subnets_zone_b" {
  description = "Elasticsearch subnets Zone B"
  default = {{ elasticsearch_zone_b_subnet_blocks }}
  type = list(string)
}

variable "elasticsearch_subnets_zone_c" {
  description = "Elasticsearch subnets Zone C"
  default = {{ elasticsearch_zone_c_subnet_blocks }}
  type = list(string)
}


# Network
resource "aws_subnet" "elasticsearch_zone_a" {
  count = length(var.elasticsearch_subnets_zone_a)

  availability_zone = var.aws_availability_zones[0]
  cidr_block = var.elasticsearch_subnets_zone_a[count.index]
  vpc_id = aws_vpc.eks.id

  tags = local.tags_elasticsearch
}

resource "aws_subnet" "elasticsearch_zone_b" {
  count = length(var.elasticsearch_subnets_zone_b)

  availability_zone = var.aws_availability_zones[1]
  cidr_block = var.elasticsearch_subnets_zone_b[count.index]
  vpc_id = aws_vpc.eks.id

  tags = local.tags_elasticsearch
}

resource "aws_subnet" "elasticsearch_zone_c" {
  count = length(var.elasticsearch_subnets_zone_c)

  availability_zone = var.aws_availability_zones[2]
  cidr_block = var.elasticsearch_subnets_zone_c[count.index]
  vpc_id = aws_vpc.eks.id

  tags = local.tags_elasticsearch
}

resource "aws_route_table_association" "elasticsearch_cluster_zone_a" {
  count = length(var.elasticsearch_subnets_zone_a)

  subnet_id      = aws_subnet.elasticsearch_zone_a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "elasticsearch_cluster_zone_b" {
  count = length(var.elasticsearch_subnets_zone_b)

  subnet_id      = aws_subnet.elasticsearch_zone_b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "elasticsearch_cluster_zone_c" {
  count = length(var.elasticsearch_subnets_zone_c)

  subnet_id      = aws_subnet.elasticsearch_zone_c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_security_group" "elasticsearch" {
  name = "elasticsearch-${var.kubernetes_cluster_id}"
  description = "Elasticsearch security group"
  vpc_id = aws_vpc.eks.id

  ingress {
    from_port = 443
    to_port = 443
    protocol = "tcp"

    cidr_blocks = [
      aws_vpc.eks.cidr_block
    ]
  }

  tags = local.tags_elasticsearch
}

{%- endif -%}
