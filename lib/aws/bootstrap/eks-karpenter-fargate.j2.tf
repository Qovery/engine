{% if enable_karpenter %}
variable "eks_fargate_subnets_zone_a_private" {
  description = "EKS fargate private subnets Zone A"
  default     = ["10.0.166.0/24"] # TODO PG remove hardcoded ip range and remove when VPC provided
  type        = list(string)
}

variable "eks_fargate_subnets_zone_b_private" {
  description = "EKS fargate private subnets Zone B"
  default     = ["10.0.168.0/24"] # TODO PG remove hardcoded ip range and remove when VPC provided
  type        = list(string)
}

variable "eks_fargate_subnets_zone_c_private" {
  description = "EKS fargate private subnets Zone C"
  default     = ["10.0.170.0/24"] # TODO PG remove hardcoded ip range and remove when VPC provided
  type        = list(string)
}

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

resource "aws_eks_fargate_profile" "karpenter" {
  cluster_name           = aws_eks_cluster.eks_cluster.name
  fargate_profile_name   = "karpenter-${var.kubernetes_cluster_name}"
  pod_execution_role_arn = aws_iam_role.karpenter-fargate.arn
  subnet_ids             = flatten([aws_subnet.eks_fargate_zone_a[*].id, aws_subnet.eks_fargate_zone_b[*].id, aws_subnet.eks_fargate_zone_c[*].id])

  selector {
    namespace = "kube-system"
    labels = {
      "app.kubernetes.io/name" = "karpenter",
    }
  }
}

{% if bootstrap_on_fargate %}
resource "aws_eks_fargate_profile" "ebs_csi" {
  cluster_name           = aws_eks_cluster.eks_cluster.name
  fargate_profile_name   = "ebs_csi-${var.kubernetes_cluster_name}"
  pod_execution_role_arn = aws_iam_role.karpenter-fargate.arn
  subnet_ids             = flatten([aws_subnet.eks_fargate_zone_a[*].id, aws_subnet.eks_fargate_zone_b[*].id, aws_subnet.eks_fargate_zone_c[*].id])

  selector {
    namespace = "kube-system"
    labels = {
      "app.kubernetes.io/name" = "aws-ebs-csi-driver",
    }
  }
}


resource "aws_eks_fargate_profile" "user-mapper" {
  cluster_name           = aws_eks_cluster.eks_cluster.name
  fargate_profile_name   = "user-mapper-${var.kubernetes_cluster_name}"
  pod_execution_role_arn = aws_iam_role.karpenter-fargate.arn
  subnet_ids             = flatten([aws_subnet.eks_fargate_zone_a[*].id, aws_subnet.eks_fargate_zone_b[*].id, aws_subnet.eks_fargate_zone_c[*].id])

  selector {
    namespace = "kube-system"
    labels = {
      "app.kubernetes.io/name" = "iam-eks-user-mapper",
    }
  }
}

resource "aws_eks_fargate_profile" "core-dns" {
  cluster_name           = aws_eks_cluster.eks_cluster.name
  fargate_profile_name   = "core-dns-${var.kubernetes_cluster_name}"
  pod_execution_role_arn = aws_iam_role.karpenter-fargate.arn
  subnet_ids             = flatten([aws_subnet.eks_fargate_zone_a[*].id, aws_subnet.eks_fargate_zone_b[*].id, aws_subnet.eks_fargate_zone_c[*].id])

  selector {
    namespace = "kube-system"
    labels = {
      "k8s-app" = "kube-dns",
    }
  }
}
{% endif %}

resource "aws_iam_role" "karpenter-fargate" {
  name = "qovery-eks-fargate-profile-${var.kubernetes_cluster_id}"

  assume_role_policy = jsonencode(
    {
      Statement = [{
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "eks-fargate-pods.amazonaws.com"
        }
      }]
      Version = "2012-10-17"
    }
  )
}

resource "aws_iam_role_policy_attachment" "karpenter-AmazonEKSFargatePodExecutionRolePolicy" {
  role       = aws_iam_role.karpenter-fargate.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKSFargatePodExecutionRolePolicy"
}
{%- endif -%}