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

locals {
  tags_rds = merge(
    aws_instance.ec2_instance.tags,
    {
      "Service" = "RDS"
    }
  )
}

# Network
resource "aws_subnet" "rds_zone_a" {
  count = length(var.rds_subnets_zone_a)

  availability_zone = var.aws_availability_zones[0]
  cidr_block        = var.rds_subnets_zone_a[count.index]
  vpc_id            = aws_vpc.ec2.id

  tags = local.tags_rds
}

resource "aws_subnet" "rds_zone_b" {
  count = length(var.rds_subnets_zone_b)

  availability_zone = var.aws_availability_zones[1]
  cidr_block        = var.rds_subnets_zone_b[count.index]
  vpc_id            = aws_vpc.ec2.id

  tags = local.tags_rds
}

resource "aws_subnet" "rds_zone_c" {
  count = length(var.rds_subnets_zone_c)

  availability_zone = var.aws_availability_zones[2]
  cidr_block        = var.rds_subnets_zone_c[count.index]
  vpc_id            = aws_vpc.ec2.id

  tags = local.tags_rds
}

resource "aws_route_table_association" "rds_cluster_zone_a" {
  count = length(var.rds_subnets_zone_a)

  subnet_id      = aws_subnet.rds_zone_a.*.id[count.index]
  route_table_id = aws_route_table.ec2_instance.id
}

resource "aws_route_table_association" "rds_cluster_zone_b" {
  count = length(var.rds_subnets_zone_b)

  subnet_id      = aws_subnet.rds_zone_b.*.id[count.index]
  route_table_id = aws_route_table.ec2_instance.id
}

resource "aws_route_table_association" "rds_cluster_zone_c" {
  count = length(var.rds_subnets_zone_c)

  subnet_id      = aws_subnet.rds_zone_c.*.id[count.index]
  route_table_id = aws_route_table.ec2_instance.id
}

resource "aws_db_subnet_group" "rds" {
  description = "RDS linked to ${var.kubernetes_cluster_id}"
  name = aws_vpc.ec2.id
  subnet_ids = flatten([aws_subnet.rds_zone_a.*.id, aws_subnet.rds_zone_b.*.id, aws_subnet.rds_zone_c.*.id])

  tags = local.tags_rds
}

# IAM
resource "aws_iam_role" "rds_enhanced_monitoring" {
  name        = "qovery-rds-enhanced-monitoring-${var.kubernetes_cluster_id}"
  assume_role_policy = data.aws_iam_policy_document.rds_enhanced_monitoring.json

  tags = local.tags_rds
}

resource "aws_iam_role_policy_attachment" "rds_enhanced_monitoring" {
  role       = aws_iam_role.rds_enhanced_monitoring.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonRDSEnhancedMonitoringRole"
}