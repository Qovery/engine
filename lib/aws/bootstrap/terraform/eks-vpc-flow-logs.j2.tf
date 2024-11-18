{%- if not user_provided_network %}
{%- if aws_enable_vpc_flow_logs %}
# VPC flow logs
resource "aws_flow_log" "eks_vpc_flow_logs" {
  log_destination      = aws_s3_bucket.vpc_flow_logs.arn
  log_destination_type = "s3"
  traffic_type         = "ALL"
  vpc_id               = aws_vpc.eks.id
}
{% endif %}
{% endif %}
