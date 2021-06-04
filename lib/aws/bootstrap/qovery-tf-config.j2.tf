locals {
  qovery_tf_config = <<TF_CONFIG
{
  "cloud_provider": "${var.cloud_provider}",
  "region": "${var.region}",
  "cluster_name": "${var.kubernetes_cluster_name}",
  "cluster_id": "${var.kubernetes_cluster_id}",
  "organization_id": "${var.organization_id}",
  "test_cluster": "${var.test_cluster}",
  "aws_access_key_id": "{{ aws_access_key }}",
  "aws_secret_access_key": "{{ aws_secret_key }}",
  "external_dns_provider": "{{ external_dns_provider }}",
  "dns_email_report": "{{ dns_email_report }}",
  "acme_server_url": "{{ acme_server_url }}",
  "managed_dns_domains_terraform_format": "{{ managed_dns_domains_terraform_format }}",
  "cloudflare_api_token": "{{ cloudflare_api_token }}",
  "cloudflare_email": "{{ cloudflare_email }}",
  "feature_flag_metrics_history": "{% if metrics_history_enabled %}true{% else %}false{% endif %}",
  "aws_iam_eks_user_mapper_key": "${aws_iam_access_key.iam_eks_user_mapper.id}",
  "aws_iam_eks_user_mapper_secret": "${aws_iam_access_key.iam_eks_user_mapper.secret}",
  "aws_iam_cluster_autoscaler_key": "${aws_iam_access_key.iam_eks_cluster_autoscaler.id}",
  "aws_iam_cluster_autoscaler_secret": "${aws_iam_access_key.iam_eks_cluster_autoscaler.secret}",
  "managed_dns_resolvers_terraform_format": "{{ managed_dns_resolvers_terraform_format }}",
  "feature_flag_log_history": "{% if log_history_enabled %}true{% else %}false{% endif %}",
  "loki_storage_config_aws_s3": "s3://${urlencode(aws_iam_access_key.iam_eks_loki.id)}:${urlencode(aws_iam_access_key.iam_eks_loki.secret)}@${var.region}/${aws_s3_bucket.loki_bucket.bucket}",
  "aws_iam_loki_storage_key": "${aws_iam_access_key.iam_eks_loki.id}",
  "aws_iam_loki_storage_secret": "${aws_iam_access_key.iam_eks_loki.secret}",
  "qovery_agent_version": "${data.external.get_agent_version_to_use.result.version}",
  "qovery_engine_version": "${data.external.get_agent_version_to_use.result.version}",
  "nats_host_url": "${var.qovery_nats_url}",
  "nats_username": "${var.qovery_nats_user}",
  "nats_password": "${var.qovery_nats_password}"
}
TF_CONFIG
}

resource "local_file" "qovery_tf_config" {
  filename = "qovery-tf-config.json"
  content = local.qovery_tf_config
  file_permission = "0600"
}
