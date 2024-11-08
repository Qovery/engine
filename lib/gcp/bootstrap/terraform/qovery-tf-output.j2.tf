data "aws_caller_identity" "current" {}

{%- if log_history_enabled %}
output "loki_logging_service_account_email" { value = resource.google_service_account.loki_service_account.email }
{%- endif %}
output "gke_cluster_public_hostname" { value = google_container_cluster.primary.endpoint  }
output "kubeconfig" {
    sensitive = true
    depends_on = [google_container_cluster.primary]
    value = <<KUBECONFIG
apiVersion: v1
clusters:
- cluster:
    certificate-authority-data: ${google_container_cluster.primary.master_auth.0.cluster_ca_certificate}
    server: https://${google_container_cluster.primary.endpoint}
  name: gke_${replace(var.kubernetes_cluster_id, "-", "_")}
contexts:
- context:
    cluster: gke_${replace(var.kubernetes_cluster_id, "-", "_")}
    user: gke_${replace(var.kubernetes_cluster_id, "-", "_")}
  name: gke_${replace(var.kubernetes_cluster_id, "-", "_")}
current-context: gke_${replace(var.kubernetes_cluster_id, "-", "_")}
kind: Config
preferences: {}
users:
- name: gke_${replace(var.kubernetes_cluster_id, "-", "_")}
  user:
    exec:
      apiVersion: client.authentication.k8s.io/v1beta1
      command: gke-gcloud-auth-plugin
      installHint: Install gke-gcloud-auth-plugin for use with kubectl by following
        https://cloud.google.com/blog/products/containers-kubernetes/kubectl-auth-changes-in-gke
      provideClusterInfo: true
KUBECONFIG
}
