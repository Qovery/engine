{%- if not user_provided_network -%}

locals {
  tags_elasticache = merge(
    aws_eks_cluster.eks_cluster.tags,
    {
      "Service" = "Elasticache"
    }
  )
}

# Elasticache
variable "elasticache_subnets_zone_a" {
  description = "Elasticache subnets Zone A"
  default = {{ elasticache_zone_a_subnet_blocks }}
  type = list(string)
}

variable "elasticache_subnets_zone_b" {
  description = "Elasticache subnets Zone B"
  default = {{ elasticache_zone_b_subnet_blocks }}
  type = list(string)
}

variable "elasticache_subnets_zone_c" {
  description = "Elasticache subnets Zone C"
  default = {{ elasticache_zone_c_subnet_blocks }}
  type = list(string)
}

# Network
resource "aws_subnet" "elasticache_zone_a" {
  count = length(var.elasticache_subnets_zone_a)

  availability_zone = var.aws_availability_zones[0]
  cidr_block = var.elasticache_subnets_zone_a[count.index]
  vpc_id = aws_vpc.eks.id

  tags = local.tags_elasticache
}

resource "aws_subnet" "elasticache_zone_b" {
  count = length(var.elasticache_subnets_zone_b)

  availability_zone = var.aws_availability_zones[1]
  cidr_block = var.elasticache_subnets_zone_b[count.index]
  vpc_id = aws_vpc.eks.id

  tags = local.tags_elasticache
}

resource "aws_subnet" "elasticache_zone_c" {
  count = length(var.elasticache_subnets_zone_c)

  availability_zone = var.aws_availability_zones[2]
  cidr_block = var.elasticache_subnets_zone_c[count.index]
  vpc_id = aws_vpc.eks.id

  tags = local.tags_elasticache
}

resource "aws_route_table_association" "elasticache_cluster_zone_a" {
  count = length(var.elasticache_subnets_zone_a)

  subnet_id      = aws_subnet.elasticache_zone_a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "elasticache_cluster_zone_b" {
  count = length(var.elasticache_subnets_zone_b)

  subnet_id      = aws_subnet.elasticache_zone_b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "elasticache_cluster_zone_c" {
  count = length(var.elasticache_subnets_zone_c)

  subnet_id      = aws_subnet.elasticache_zone_c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_elasticache_subnet_group" "elasticache" {
  description = "Elasticache linked to ${var.kubernetes_cluster_id}"
  # WARNING: this "name" value is used into elasticache clusters, you need to update it accordingly
  name = "elasticache-${aws_vpc.eks.id}"
  subnet_ids = flatten([aws_subnet.elasticache_zone_a.*.id, aws_subnet.elasticache_zone_b.*.id, aws_subnet.elasticache_zone_c.*.id])
}

# Todo: create a bastion to avoid this

resource "aws_security_group_rule" "elasticache_remote_access" {
  cidr_blocks       = ["0.0.0.0/0"]
  description       = "Allow Redis incoming access from anywhere"
  from_port         = 6379
  protocol          = "tcp"
  security_group_id = aws_security_group.eks_cluster_workers.id
  to_port           = 6379
  type              = "ingress"
}

{%- endif -%}
