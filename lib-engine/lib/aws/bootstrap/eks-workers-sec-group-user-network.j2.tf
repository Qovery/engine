{%- if user_provided_network -%}

##############################
# Worker Node Security Group #
##############################

resource "aws_security_group" "eks_cluster_workers" {
  name        = "qovery-eks-workers-${var.kubernetes_cluster_id}"
  description = "Security group for all nodes in the cluster"
  vpc_id      = data.aws_vpc.eks.id

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
    ipv6_cidr_blocks = ["::/0"]
  }

  ingress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
    ipv6_cidr_blocks = ["::/0"]
  }

  tags = merge(
    local.tags_eks,
    {
      Name = "qovery-eks-workers",
      "kubernetes.io/cluster/${var.kubernetes_cluster_id}" = "owned",
    }
  )
}

{%- endif -%}
