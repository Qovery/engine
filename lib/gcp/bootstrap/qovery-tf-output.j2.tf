data "aws_caller_identity" "current" {}

{%- if log_history_enabled %}
output "loki_logging_service_account_email" { value = resource.google_service_account.loki_service_account.email }
{%- endif %}
output "gke_cluster_public_hostname" { value = google_container_cluster.primary.endpoint  }
