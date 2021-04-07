resource "helm_release" "k8s_token_rotate" {
  name = "k8s-token-rotate"
  chart = "charts/do-k8s-token-rotate"
  namespace = "kube-system"
  atomic = true
  max_history = 50
  force_update = true

  set {
    name = "environmentVariables.DO_API_TOKEN"
    value = "{{ digitalocean_token }}"
  }

  set {
    name = "environmentVariables.SPACES_KEY_ACCESS"
    value = "{{ spaces_access_id }}"
  }

  set {
    name = "environmentVariables.SPACES_SECRET_KEY"
    value = "{{ spaces_secret_key }}"
  }

  set {
    name = "environmentVariables.SPACES_BUCKET"
    value = digitalocean_spaces_bucket.space_bucket_kubeconfig.name
  }

  set {
    name = "environmentVariables.SPACES_REGION"
    value = var.region
  }

  set {
    name = "environmentVariables.SPACES_FILENAME"
    value = digitalocean_spaces_bucket_object.upload_kubeconfig.key
  }

  set {
    name = "environmentVariables.K8S_CLUSTER_ID"
    value = digitalocean_kubernetes_cluster.kubernetes_cluster.id
  }

  set {
    name = "forced_upgrade"
    value = var.forced_upgrade
  }

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster
  ]
}
