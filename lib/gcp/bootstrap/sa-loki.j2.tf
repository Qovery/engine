{%- if log_history_enabled %}

locals {
  loki_service_account_name = "loki-logging-${var.kubernetes_cluster_name}"
}

resource "google_service_account" "loki_service_account" {
  account_id   = local.loki_service_account_name
  display_name = "Service account for Loki"
  project      =  var.project_id
}

resource "google_project_iam_binding" "project" {
  project = var.project_id
  role    = "roles/storage.objectAdmin"

  members = [
    "serviceAccount:${resource.google_service_account.loki_service_account.email}"
  ]
}

resource "google_service_account_iam_binding" "loki-workload-identity" {
  service_account_id = resource.google_service_account.loki_service_account.name
  role               = "roles/iam.workloadIdentityUser"

  members = [
    "serviceAccount:${var.project_id}.svc.id.goog[qovery/loki]",
  ]
}

{%- endif %}