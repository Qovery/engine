data "aws_iam_policy_document" "rds_enhanced_monitoring" {
  statement {
    actions = [
      "sts:AssumeRole",
    ]

    effect = "Allow"

    principals {
      type        = "Service"
      identifiers = ["monitoring.rds.amazonaws.com"]
    }
  }
}

# Network
resource "aws_subnet" "rds-zone-a" {
  count = var.rds_nb_subnets_per_zone

  availability_zone = data.aws_availability_zones.available.names[0]
  cidr_block        = "10.0.${count.index * 2 + 214}.0/${var.rds_cidr_subnet}"
  vpc_id            = aws_vpc.eks.id
  tags = map(
    "Name", "${var.region_cluster_name}-rds",
    "ClusterName", var.cluster_name,
    "RegionClusterName", var.region_cluster_name,
    "Region", var.region,
    "Service", "RDS"
  )
}

resource "aws_subnet" "rds-zone-b" {
  count = var.rds_nb_subnets_per_zone

  availability_zone = data.aws_availability_zones.available.names[1]
  cidr_block        = "10.0.${count.index * 2 + 228}.0/${var.rds_cidr_subnet}"
  vpc_id            = aws_vpc.eks.id
  tags = map(
    "Name", "${var.region_cluster_name}-rds",
    "ClusterName", var.cluster_name,
    "RegionClusterName", var.region_cluster_name,
    "Region", var.region,
    "Service", "RDS"
  )
}

resource "aws_subnet" "rds-zone-c" {
  count = var.rds_nb_subnets_per_zone - 1

  availability_zone = data.aws_availability_zones.available.names[2]
  cidr_block        = "10.0.${count.index * 2 + 242}.0/${var.rds_cidr_subnet}"
  vpc_id            = aws_vpc.eks.id
  tags = map(
    "Name", "${var.region_cluster_name}-rds",
    "ClusterName", var.cluster_name,
    "RegionClusterName", var.region_cluster_name,
    "Region", var.region,
    "Service", "RDS"
  )
}

resource "aws_route_table_association" "rds_cluster-zone-a" {
  count = var.rds_nb_subnets_per_zone

  subnet_id      = aws_subnet.rds-zone-a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "rds_cluster-zone-b" {
  count = var.rds_nb_subnets_per_zone

  subnet_id      = aws_subnet.rds-zone-b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "rds_cluster-zone-c" {
  count = var.rds_nb_subnets_per_zone - 1

  subnet_id      = aws_subnet.rds-zone-c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_db_subnet_group" "rds" {
  description = "RDS linked to ${var.region_cluster_name}"
  name = aws_vpc.eks.id
  subnet_ids = flatten([aws_subnet.rds-zone-a.*.id, aws_subnet.rds-zone-b.*.id, aws_subnet.rds-zone-c.*.id])
  tags = {
    ClusterName = var.cluster_name
    RegionClusterName = var.region_cluster_name
    Region = var.region
    Service = "RDS"
  }
}

# IAM
resource "aws_iam_role" "rds_enhanced_monitoring" {
  name        = "${var.region_cluster_name}-rds-enhanced-monitoring"
  assume_role_policy = data.aws_iam_policy_document.rds_enhanced_monitoring.json
}

resource "aws_iam_role_policy_attachment" "rds_enhanced_monitoring" {
  role       = aws_iam_role.rds_enhanced_monitoring.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonRDSEnhancedMonitoringRole"
}

# Todo: create a bastion to avoid this

resource "aws_security_group_rule" "postgres_remote_access" {
  count = var.test_cluster == "false" ? 1 : 0
  cidr_blocks       = ["0.0.0.0/0"]
  description       = "Allow RDS PostgreSQL incoming access from anywhere"
  from_port         = 5432
  protocol          = "tcp"
  security_group_id = aws_security_group.eks_cluster_workers.id
  to_port           = 5432
  type              = "ingress"
}

resource "aws_security_group_rule" "mysql_remote_access" {
  count = var.test_cluster == "false" ? 1 : 0
  cidr_blocks       = ["0.0.0.0/0"]
  description       = "Allow RDS MySQL incoming access from anywhere"
  from_port         = 3306
  protocol          = "tcp"
  security_group_id = aws_security_group.eks_cluster_workers.id
  to_port           = 3306
  type              = "ingress"
}
