{%- if not user_provided_network -%}

resource "aws_security_group" "eks_cluster" {
  name        = "qovery-eks-${var.kubernetes_cluster_id}"
  description = "Cluster communication from control plane to worker nodes"
  vpc_id      = aws_vpc.eks.id

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = local.tags_eks
}

{%- endif -%}
