{% set rds_enabled = rds_subnets_zone_a_ids | default(value=[]) | length  %}
{%- if user_provided_network and rds_enabled -%}

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
    aws_eks_cluster.eks_cluster.tags,
    {
      "Service" = "RDS"
    }
  )
}


# Network is provided by the user
variable "rds_subnets_zone_a_ids" {
  type    = list(string)
  default = [
    {%- for id in rds_subnets_zone_a_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

variable "rds_subnets_zone_b_ids" {
  type    = list(string)
  default = [
    {%- for id in rds_subnets_zone_b_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

variable "rds_subnets_zone_c_ids" {
  type    = list(string)
  default = [
    {%- for id in rds_subnets_zone_c_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}


# Network
data "aws_subnet" "rds_zone_a" {
  count = length(var.rds_subnets_zone_a_ids)
  id    = var.rds_subnets_zone_a_ids[count.index]
}

data "aws_subnet" "rds_zone_b" {
  count = length(var.rds_subnets_zone_b_ids)
  id    = var.rds_subnets_zone_b_ids[count.index]
}

data "aws_subnet" "rds_zone_c" {
  count = length(var.rds_subnets_zone_c_ids)
  id    = var.rds_subnets_zone_c_ids[count.index]
}

#resource "aws_route_table_association" "rds_cluster_zone_a" {
#  count = length(var.rds_subnets_zone_a_ids)
#
#  subnet_id      = data.aws_subnet.rds_zone_a.*.id[count.index]
#  route_table_id = aws_route_table.eks_cluster.id
#}
#
#resource "aws_route_table_association" "rds_cluster_zone_b" {
#  count = length(var.rds_subnets_zone_b_ids)
#
#  subnet_id      = data.aws_subnet.rds_zone_b.*.id[count.index]
#  route_table_id = aws_route_table.eks_cluster.id
#}
#
#resource "aws_route_table_association" "rds_cluster_zone_c" {
#  count = length(var.rds_subnets_zone_c_ids)
#
#  subnet_id      = data.aws_subnet.rds_zone_c.*.id[count.index]
#  route_table_id = aws_route_table.eks_cluster.id
#}

resource "aws_db_subnet_group" "rds" {
  description = "RDS linked to ${var.kubernetes_cluster_id}"
  name = data.aws_vpc.eks.id
  subnet_ids = flatten([data.aws_subnet.rds_zone_a.*.id, data.aws_subnet.rds_zone_b.*.id, data.aws_subnet.rds_zone_c.*.id])

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

{%- endif -%}
