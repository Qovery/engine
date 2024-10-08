{%- if user_provided_network -%}

resource "aws_security_group" "eks_cluster" {
  name        = "qovery-eks-${var.kubernetes_cluster_id}"
  description = "Cluster communication from control plane to worker nodes"
  vpc_id      = data.aws_vpc.eks.id

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
    ipv6_cidr_blocks = ["::/0"]
  }

  tags = local.tags_eks
}

{%- endif -%}
