# Network

resource "aws_subnet" "ddb-zone-a" {
  count = var.ddb_nb_subnets_per_zone

  availability_zone = data.aws_availability_zones.available.names[0]
  cidr_block = "10.0.${count.index * 2 + 196}.0/${var.ddb_cidr_subnet}"
  vpc_id = aws_vpc.eks.id
  tags = map(
    "Name", "${var.region_cluster_name}-ddb",
    "ClusterName", var.cluster_name,
    "RegionClusterName", var.region_cluster_name,
    "Region", var.region,
    "Service", "DocumentDB"
  )
}

resource "aws_subnet" "ddb-zone-b" {
  count = var.ddb_nb_subnets_per_zone

  availability_zone = data.aws_availability_zones.available.names[1]
  cidr_block = "10.0.${count.index * 2 + 202}.0/${var.ddb_cidr_subnet}"
  vpc_id = aws_vpc.eks.id
  tags = map(
    "Name", "${var.region_cluster_name}-ddb",
    "ClusterName", var.cluster_name,
    "RegionClusterName", var.region_cluster_name,
    "Region", var.region,
    "Service", "DocumentDB"
  )
}

resource "aws_subnet" "ddb-zone-c" {
  count = var.ddb_nb_subnets_per_zone

  availability_zone = data.aws_availability_zones.available.names[2]
  cidr_block = "10.0.${count.index * 2 + 208}.0/${var.ddb_cidr_subnet}"
  vpc_id = aws_vpc.eks.id
  tags = map(
    "Name", "${var.region_cluster_name}-ddb",
    "ClusterName", var.cluster_name,
    "RegionClusterName", var.region_cluster_name,
    "Region", var.region,
    "Service", "DocumentDB"
  )
}

resource "aws_route_table_association" "ddb_cluster-zone-a" {
  count = var.ddb_nb_subnets_per_zone

  subnet_id      = aws_subnet.ddb-zone-a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "ddb_cluster-zone-b" {
  count = var.ddb_nb_subnets_per_zone

  subnet_id      = aws_subnet.ddb-zone-b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "ddb_cluster-zone-c" {
  count = var.ddb_nb_subnets_per_zone - 1

  subnet_id      = aws_subnet.ddb-zone-c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_docdb_subnet_group" "ddb" {
  description = "DocumentDB linked to ${var.region_cluster_name}"
  name = "${aws_vpc.eks.id}-ddb"
  subnet_ids = flatten([aws_subnet.ddb-zone-a.*.id, aws_subnet.ddb-zone-b.*.id, aws_subnet.ddb-zone-c.*.id])
  tags = {
    ClusterName = var.cluster_name
    RegionClusterName = var.region_cluster_name
    Region = var.region
    Service = "DocumentDB"
  }
}

# Todo: create a bastion to avoid this

resource "aws_security_group_rule" "documentdb_remote_access" {
  count = var.test_cluster == "false" ? 1 : 0
  cidr_blocks       = ["0.0.0.0/0"]
  description       = "Allow DocumentDB incoming access from anywhere"
  from_port         = 27017
  protocol          = "tcp"
  security_group_id = aws_security_group.eks_cluster_workers.id
  to_port           = 27017
  type              = "ingress"
}
