{% if enable_karpenter %}

{% if user_provided_network %}
variable "eks_karpenter_fargate_subnets_zone_a_ids" {
  type    = list(string)
  default = [
    {%- for id in eks_karpenter_fargate_subnets_zone_a_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

variable "eks_karpenter_fargate_subnets_zone_b_ids" {
  type    = list(string)
  default = [
    {%- for id in eks_karpenter_fargate_subnets_zone_b_ids -%}
    "{{ id }}",
    {%- endfor -%}
  ]
}

variable "eks_karpenter_fargate_subnets_zone_c_ids" {
  type    = list(string)
  default = [
    {%- for id in eks_karpenter_fargate_subnets_zone_c_ids -%}
    "{{ id }}",
    {%- endfor -%}
    ]
}

data "aws_subnet" "eks_fargate_zone_a" {
  count = length(var.eks_karpenter_fargate_subnets_zone_a_ids)
  id    = var.eks_karpenter_fargate_subnets_zone_a_ids[count.index]
}

data "aws_subnet" "eks_fargate_zone_b" {
  count = length(var.eks_karpenter_fargate_subnets_zone_b_ids)
  id    = var.eks_karpenter_fargate_subnets_zone_b_ids[count.index]
}

data "aws_subnet" "eks_fargate_zone_c" {
  count = length(var.eks_karpenter_fargate_subnets_zone_c_ids)
  id    = var.eks_karpenter_fargate_subnets_zone_c_ids[count.index]
}


{% endif %}

resource "aws_eks_fargate_profile" "karpenter" {
  cluster_name           = aws_eks_cluster.eks_cluster.name
  fargate_profile_name   = "karpenter-${var.kubernetes_cluster_name}"
  pod_execution_role_arn = aws_iam_role.karpenter-fargate.arn
{% if user_provided_network %}
  subnet_ids             = flatten([data.aws_subnet.eks_fargate_zone_a[*].id, data.aws_subnet.eks_fargate_zone_b[*].id, data.aws_subnet.eks_fargate_zone_c[*].id])
{% elif vpc_qovery_network_mode == "WithNatGateways" %}
  subnet_ids             = flatten([aws_subnet.eks_fargate_zone_a[*].id, aws_subnet.eks_fargate_zone_b[*].id, aws_subnet.eks_fargate_zone_c[*].id])
{% else %}
  subnet_ids             = flatten([aws_subnet.eks_fargate_zone_a[*].id])
{% endif %}
 tags                   = local.tags_eks

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

{% if user_provided_network %}
  subnet_ids             = flatten([data.aws_subnet.eks_fargate_zone_a[*].id, data.aws_subnet.eks_fargate_zone_b[*].id, data.aws_subnet.eks_fargate_zone_c[*].id])
{% elif vpc_qovery_network_mode == "WithNatGateways" %}
  subnet_ids             = flatten([aws_subnet.eks_fargate_zone_a[*].id, aws_subnet.eks_fargate_zone_b[*].id, aws_subnet.eks_fargate_zone_c[*].id])
{% else %}
  subnet_ids             = flatten([aws_subnet.eks_fargate_zone_a[*].id])
{% endif %}
  tags                   = local.tags_eks

  selector {
    namespace = "kube-system"
    labels = {
      "app.kubernetes.io/name" = "aws-ebs-csi-driver",
    }
  }
}

resource "aws_eks_fargate_profile" "core-dns" {
  cluster_name           = aws_eks_cluster.eks_cluster.name
  fargate_profile_name   = "core-dns-${var.kubernetes_cluster_name}"
  pod_execution_role_arn = aws_iam_role.karpenter-fargate.arn
{% if user_provided_network %}
  subnet_ids             = flatten([data.aws_subnet.eks_fargate_zone_a[*].id, data.aws_subnet.eks_fargate_zone_b[*].id, data.aws_subnet.eks_fargate_zone_c[*].id])
{% elif vpc_qovery_network_mode == "WithNatGateways" %}
  subnet_ids             = flatten([aws_subnet.eks_fargate_zone_a[*].id, aws_subnet.eks_fargate_zone_b[*].id, aws_subnet.eks_fargate_zone_c[*].id])
{% else %}
  subnet_ids             = flatten([aws_subnet.eks_fargate_zone_a[*].id])
{% endif %}
  tags                   = local.tags_eks

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
  tags = local.tags_eks

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
