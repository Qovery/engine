{%- if enable_keda %}
locals {
  keda_operator_service_account_name      = "keda-operator-${var.kubernetes_cluster_name}"
  keda_metrics_server_service_account_name = "keda-metrics-${var.kubernetes_cluster_name}"
}

# KEDA Operator Service Account + Workload Identity
resource "google_service_account" "keda_operator_service_account" {
  account_id   = local.keda_operator_service_account_name
  display_name = "Service account for KEDA operator for cluster ${var.kubernetes_cluster_name}"
  project      = var.project_id
  description  = jsonencode(local.minimal_tags_common)
}

resource "google_service_account_iam_binding" "keda_operator_workload_identity" {
  service_account_id = resource.google_service_account.keda_operator_service_account.name
  role               = "roles/iam.workloadIdentityUser"
  members = [
    "serviceAccount:${var.project_id}.svc.id.goog[qovery/keda-operator]",
  ]
}

# KEDA Metrics Server Service Account + Workload Identity
resource "google_service_account" "keda_metrics_server_service_account" {
  account_id   = local.keda_metrics_server_service_account_name
  display_name = "Service account for KEDA metrics server for cluster ${var.kubernetes_cluster_name}"
  project      = var.project_id
  description  = jsonencode(local.minimal_tags_common)
}

resource "google_service_account_iam_binding" "keda_metrics_server_workload_identity" {
  service_account_id = resource.google_service_account.keda_metrics_server_service_account.name
  role               = "roles/iam.workloadIdentityUser"
  members = [
    "serviceAccount:${var.project_id}.svc.id.goog[qovery/keda-metrics-server]",
  ]
}
{%- endif %}
