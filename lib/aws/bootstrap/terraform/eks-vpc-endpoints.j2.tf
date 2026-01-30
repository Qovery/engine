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
