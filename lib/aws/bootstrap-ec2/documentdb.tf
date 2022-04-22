locals {
  tags_documentdb = merge(
  aws_ec2_cluster.ec2_cluster.tags,
  {
    "Service" = "DocumentDB"
  }
  )
}

# Network

resource "aws_subnet" "documentdb_zone_a" {
  count = length(var.documentdb_subnets_zone_a)

  availability_zone = var.aws_availability_zones[0]
  cidr_block = var.documentdb_subnets_zone_a[count.index]
  vpc_id = aws_vpc.ec2.id

  tags = local.tags_documentdb
}

resource "aws_subnet" "documentdb_zone_b" {
  count = length(var.documentdb_subnets_zone_b)

  availability_zone = var.aws_availability_zones[1]
  cidr_block = var.documentdb_subnets_zone_b[count.index]
  vpc_id = aws_vpc.ec2.id

  tags = local.tags_documentdb
}

resource "aws_subnet" "documentdb_zone_c" {
  count = length(var.documentdb_subnets_zone_c)

  availability_zone = var.aws_availability_zones[2]
  cidr_block = var.documentdb_subnets_zone_c[count.index]
  vpc_id = aws_vpc.ec2.id

  tags = local.tags_documentdb
}

resource "aws_route_table_association" "documentdb_cluster_zone_a" {
  count = length(var.documentdb_subnets_zone_a)

  subnet_id      = aws_subnet.documentdb_zone_a.*.id[count.index]
  route_table_id = aws_route_table.ec2_cluster.id
}

resource "aws_route_table_association" "documentdb_cluster_zone_b" {
  count = length(var.documentdb_subnets_zone_b)

  subnet_id      = aws_subnet.documentdb_zone_b.*.id[count.index]
  route_table_id = aws_route_table.ec2_cluster.id
}

resource "aws_route_table_association" "documentdb_cluster_zone_c" {
  count = length(var.documentdb_subnets_zone_c)

  subnet_id      = aws_subnet.documentdb_zone_c.*.id[count.index]
  route_table_id = aws_route_table.ec2_cluster.id
}

resource "aws_docdb_subnet_group" "documentdb" {
  description = "DocumentDB linked to ${var.kubernetes_cluster_id}"
  name = "documentdb-${aws_vpc.ec2.id}"
  subnet_ids = flatten([aws_subnet.documentdb_zone_a.*.id, aws_subnet.documentdb_zone_b.*.id, aws_subnet.documentdb_zone_c.*.id])

  tags = local.tags_documentdb
}

# Todo: create a bastion to avoid this

resource "aws_security_group_rule" "documentdb_remote_access" {
  cidr_blocks       = ["0.0.0.0/0"]
  description       = "Allow DocumentDB incoming access from anywhere"
  from_port         = 27017
  protocol          = "tcp"
  security_group_id = aws_security_group.ec2_cluster_workers.id
  to_port           = 27017
  type              = "ingress"
}
