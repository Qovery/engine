{% if enable_karpenter and not user_provided_network %}
variable "eks_fargate_subnets_zone_a_private" {
  description = "EKS fargate private subnets Zone A"
  default     = {{ fargate_profile_zone_a_subnet_blocks }}
  type        = list(string)
}

{% if enable_karpenter and  vpc_qovery_network_mode == "WithNatGateways" %}
variable "eks_fargate_subnets_zone_b_private" {
  description = "EKS fargate private subnets Zone B"
  default     = {{ fargate_profile_zone_b_subnet_blocks }}
  type        = list(string)
}

variable "eks_fargate_subnets_zone_c_private" {
  description = "EKS fargate private subnets Zone C"
  default     = {{ fargate_profile_zone_c_subnet_blocks }}
  type        = list(string)
}
{% endif %}

# Private subnets
resource "aws_subnet" "eks_fargate_zone_a" {
  availability_zone       = var.aws_availability_zones[0]
  cidr_block              = var.eks_fargate_subnets_zone_a_private[0]
  vpc_id                  = aws_vpc.eks.id
  map_public_ip_on_launch = false

  tags = merge(
    local.tags_common,
    {
      "Service"                = "EKS"
      "karpenter.sh/discovery" = var.kubernetes_cluster_name
    }
  )
}

{% if enable_karpenter and  vpc_qovery_network_mode == "WithNatGateways" %}
resource "aws_subnet" "eks_fargate_zone_b" {
  availability_zone       = var.aws_availability_zones[1]
  cidr_block              = var.eks_fargate_subnets_zone_b_private[0]
  vpc_id                  = aws_vpc.eks.id
  map_public_ip_on_launch = false

  tags = merge(
    local.tags_common,
    {
      "Service"                = "EKS"
      "karpenter.sh/discovery" = var.kubernetes_cluster_name
    }
  )
}

resource "aws_subnet" "eks_fargate_zone_c" {
  availability_zone       = var.aws_availability_zones[2]
  cidr_block              = var.eks_fargate_subnets_zone_c_private[0]
  vpc_id                  = aws_vpc.eks.id
  map_public_ip_on_launch = false

  tags = merge(
    local.tags_common,
    {
      "Service"                = "EKS"
      "karpenter.sh/discovery" = var.kubernetes_cluster_name
    }
  )
}
{% endif %}

# Use private route table for private subnets, Fargate doesn't allow public routes
resource "aws_route_table_association" "eks_fargate_cluster_zone_a" {
  count = length(var.eks_fargate_subnets_zone_a_private)

  subnet_id = aws_subnet.eks_fargate_zone_a.*.id[count.index]
{% if vpc_qovery_network_mode == "WithoutNatGateways" %}
  route_table_id = aws_route_table.eks_karpenter_cluster_zone_a_private[count.index].id
{% elif vpc_qovery_network_mode == "WithNatGateways" %}
  route_table_id = aws_route_table.eks_cluster_zone_a_private[count.index].id
{% endif %}
}

{% if enable_karpenter and  vpc_qovery_network_mode == "WithNatGateways" %}
resource "aws_route_table_association" "eks_fargate_cluster_zone_b" {
  count = length(var.eks_fargate_subnets_zone_b_private)

  subnet_id = aws_subnet.eks_fargate_zone_b.*.id[count.index]
{% if vpc_qovery_network_mode == "WithoutNatGateways" %}
  route_table_id = aws_route_table.eks_karpenter_cluster_zone_b_private[count.index].id
{% elif vpc_qovery_network_mode == "WithNatGateways" %}
  route_table_id = aws_route_table.eks_cluster_zone_b_private[count.index].id
{% endif %}
}

resource "aws_route_table_association" "eks_fargate_cluster_zone_c" {
  count = length(var.eks_fargate_subnets_zone_c_private)

  subnet_id = aws_subnet.eks_fargate_zone_c.*.id[count.index]
{% if vpc_qovery_network_mode == "WithoutNatGateways" %}
  route_table_id = aws_route_table.eks_karpenter_cluster_zone_c_private[count.index].id
{% elif vpc_qovery_network_mode == "WithNatGateways" %}
  route_table_id = aws_route_table.eks_cluster_zone_c_private[count.index].id
{% endif %}
}
{% endif %}
{% endif %}
