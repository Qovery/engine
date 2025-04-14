locals {
  thanos_service_account_name = "thanos-${var.kubernetes_cluster_name}"
}

resource "google_service_account" "thanos_service_account" {
  account_id   = local.thanos_service_account_name
  display_name = "Service account for Prometheus-Thanos for cluster ${var.kubernetes_cluster_name}"
  project      = var.project_id
  description  = jsonencode(local.minimal_tags_common)
}

resource "google_service_account_iam_binding" "thanos-workload-identity" {
  service_account_id = resource.google_service_account.thanos_service_account.name
  role               = "roles/iam.workloadIdentityUser"

  members = [
    "serviceAccount:${var.project_id}.svc.id.goog[qovery/thanos-storegateway]",
    "serviceAccount:${var.project_id}.svc.id.goog[qovery/thanos-compactor]",
    "serviceAccount:${var.project_id}.svc.id.goog[qovery/thanos-bucketweb]",
    "serviceAccount:${var.project_id}.svc.id.goog[qovery/kube-prometheus-stack-prometheus]",
  ]
}

resource "google_storage_bucket_iam_member" "thanos_bucket_permissions" {
  bucket = "{{ thanos_gcs_bucket_name}}"
  role   = "roles/storage.objectAdmin"
  member = "serviceAccount:${resource.google_service_account.thanos_service_account.email}"
}
