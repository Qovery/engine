{%- if not user_provided_network -%}

locals {
  tags_documentdb = merge(
  aws_eks_cluster.eks_cluster.tags,
  {
    "Service" = "DocumentDB"
  }
  )
}

variable "documentdb_subnets_zone_a" {
  description = "DocumentDB subnets Zone A"
  default = {{ documentdb_zone_a_subnet_blocks }}
  type = list(string)
}

variable "documentdb_subnets_zone_b" {
  description = "DocumentDB subnets Zone B"
  default = {{ documentdb_zone_b_subnet_blocks }}
  type = list(string)
}

variable "documentdb_subnets_zone_c" {
  description = "DocumentDB subnets Zone C"
  default = {{ documentdb_zone_c_subnet_blocks }}
  type = list(string)
}

# Network
resource "aws_subnet" "documentdb_zone_a" {
  count = length(var.documentdb_subnets_zone_a)

  availability_zone = var.aws_availability_zones[0]
  cidr_block = var.documentdb_subnets_zone_a[count.index]
  vpc_id = aws_vpc.eks.id

  tags = local.tags_documentdb
}

resource "aws_subnet" "documentdb_zone_b" {
  count = length(var.documentdb_subnets_zone_b)

  availability_zone = var.aws_availability_zones[1]
  cidr_block = var.documentdb_subnets_zone_b[count.index]
  vpc_id = aws_vpc.eks.id

  tags = local.tags_documentdb
}

resource "aws_subnet" "documentdb_zone_c" {
  count = length(var.documentdb_subnets_zone_c)

  availability_zone = var.aws_availability_zones[2]
  cidr_block = var.documentdb_subnets_zone_c[count.index]
  vpc_id = aws_vpc.eks.id

  tags = local.tags_documentdb
}

resource "aws_route_table_association" "documentdb_cluster_zone_a" {
  count = length(var.documentdb_subnets_zone_a)

  subnet_id      = aws_subnet.documentdb_zone_a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "documentdb_cluster_zone_b" {
  count = length(var.documentdb_subnets_zone_b)

  subnet_id      = aws_subnet.documentdb_zone_b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "documentdb_cluster_zone_c" {
  count = length(var.documentdb_subnets_zone_c)

  subnet_id      = aws_subnet.documentdb_zone_c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_docdb_subnet_group" "documentdb" {
  description = "DocumentDB linked to ${var.kubernetes_cluster_id}"
  name = "documentdb-${aws_vpc.eks.id}"
  subnet_ids = flatten([aws_subnet.documentdb_zone_a.*.id, aws_subnet.documentdb_zone_b.*.id, aws_subnet.documentdb_zone_c.*.id])

  tags = local.tags_documentdb
}

# Todo: create a bastion to avoid this

resource "aws_security_group_rule" "documentdb_remote_access" {
  cidr_blocks       = ["0.0.0.0/0"]
  description       = "Allow DocumentDB incoming access from anywhere"
  from_port         = 27017
  protocol          = "tcp"
  security_group_id = aws_security_group.eks_cluster_workers.id
  to_port           = 27017
  type              = "ingress"
}

{%- endif -%}
