data "aws_caller_identity" "current" {}

locals {
  qovery_tf_config = <<TF_CONFIG
{
  {%- if log_history_enabled %}
  "loki_logging_service_account_email": "${resource.google_service_account.loki_service_account.email}",
  {%- endif %}
  "gke_cluster_public_hostname": "${google_container_cluster.primary.endpoint}"
}
TF_CONFIG
}

resource "local_file" "qovery_tf_config" {
  filename        = "qovery-tf-config.json"
  content         = local.qovery_tf_config
  file_permission = "0644"
}
