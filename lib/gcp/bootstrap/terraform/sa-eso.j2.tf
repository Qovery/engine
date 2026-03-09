{%- if enable_automatic_external_secrets_access %}
locals {
  eso_operator_service_account_name = "eso-operator-${var.kubernetes_cluster_name}"
}

# External Secrets Operator Service Account
resource "google_service_account" "external_secrets_operator_service_account" {
  account_id   = local.eso_operator_service_account_name
  display_name = "Service account for ESO operator for cluster ${var.kubernetes_cluster_name}"
  project      = var.project_id
  description  = jsonencode(local.minimal_tags_common)
}

# Workload Identity
resource "google_service_account_iam_binding" "external_secrets_operator_workload_identity" {
  service_account_id = google_service_account.external_secrets_operator_service_account.name
  role               = "roles/iam.workloadIdentityUser"
  members = [
    "serviceAccount:${var.project_id}.svc.id.goog[qovery/external-secrets-operator-sa]",
  ]
}

resource "google_project_iam_binding" "external_secrets_operator_secret_access" {
  project = var.project_id
  role    = "roles/secretmanager.secretAccessor"
  members = [
    "serviceAccount:${google_service_account.external_secrets_operator_service_account.email}",
  ]
}

{%- endif %}
