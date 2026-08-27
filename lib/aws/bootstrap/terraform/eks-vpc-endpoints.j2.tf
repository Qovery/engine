# VPC Endpoints
# S3 Gateway endpoint allows private access to S3 without NAT Gateway costs
# and improves security by keeping traffic within AWS network

{% if not user_provided_network %}

# IMPORTANT: AWS allows only ONE S3 Gateway endpoint per VPC
# If an S3 Gateway endpoint already exists in this VPC (e.g., created manually),
# Terraform will fail with a conflict error during apply.
# Resolution: Import the existing endpoint with:
#   terraform import 'aws_vpc_endpoint.s3' <endpoint-id>

{% if vpc_qovery_network_mode == "WithNatGateways" %}
resource "aws_vpc_endpoint" "s3" {
  vpc_id       = aws_vpc.eks.id
  service_name = "com.amazonaws.${var.region}.s3"

  route_table_ids = concat(
    [aws_route_table.eks_cluster.id],
    aws_route_table.eks_cluster_zone_a_private[*].id,
    aws_route_table.eks_cluster_zone_b_private[*].id,
    aws_route_table.eks_cluster_zone_c_private[*].id
  )

  tags = merge(
    local.tags_eks_vpc,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-s3-endpoint"
    }
  )
}
{% endif %}

{% if vpc_qovery_network_mode == "WithoutNatGateways" %}
resource "aws_vpc_endpoint" "s3" {
  vpc_id       = aws_vpc.eks.id
  service_name = "com.amazonaws.${var.region}.s3"

  route_table_ids = [aws_route_table.eks_cluster.id]

  tags = merge(
    local.tags_eks_vpc,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-s3-endpoint"
    }
  )
}
{% endif %}

{% endif %}

{% if aws_ecr_enable_pull_through_cache %}

# ECR interface endpoints keep authentication, manifests, and registry traffic
# inside the VPC. Image layers use the S3 Gateway endpoint declared above.
resource "aws_security_group" "ecr_vpc_endpoints" {
  name        = "qovery-${var.kubernetes_cluster_id}-ecr-endpoints"
  description = "Allow HTTPS access to the ECR VPC endpoints"
  vpc_id      = aws_vpc.eks.id

  ingress {
    description = "HTTPS from the cluster VPC"
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = [var.vpc_cidr_block]
  }

  tags = merge(
    local.tags_eks_vpc,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-ecr-endpoints"
    }
  )
}

resource "aws_vpc_endpoint" "ecr_api" {
  vpc_id              = aws_vpc.eks.id
  service_name        = "com.amazonaws.${var.region}.ecr.api"
  vpc_endpoint_type   = "Interface"
  private_dns_enabled = true

  # An interface endpoint accepts only one subnet per availability zone.
  subnet_ids = [
    aws_subnet.eks_zone_a[0].id,
    aws_subnet.eks_zone_b[0].id,
    aws_subnet.eks_zone_c[0].id,
  ]
  security_group_ids = [aws_security_group.ecr_vpc_endpoints.id]

  tags = merge(
    local.tags_eks_vpc,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-ecr-api-endpoint"
    }
  )
}

resource "aws_vpc_endpoint" "ecr_dkr" {
  vpc_id              = aws_vpc.eks.id
  service_name        = "com.amazonaws.${var.region}.ecr.dkr"
  vpc_endpoint_type   = "Interface"
  private_dns_enabled = true

  subnet_ids = [
    aws_subnet.eks_zone_a[0].id,
    aws_subnet.eks_zone_b[0].id,
    aws_subnet.eks_zone_c[0].id,
  ]
  security_group_ids = [aws_security_group.ecr_vpc_endpoints.id]

  tags = merge(
    local.tags_eks_vpc,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-ecr-dkr-endpoint"
    }
  )
}

{% endif %}
