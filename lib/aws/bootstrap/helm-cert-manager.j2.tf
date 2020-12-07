resource "helm_release" "cert_manager" {
  name = "cert-manager"
  chart = "common/charts/cert-manager"
  namespace = "cert-manager"
  create_namespace = true
  atomic = true
  max_history = 50

  values = [file("chart_values/cert-manager.yaml")]

  set {
    name = "fake"
    value = timestamp()
  }

  set {
    name = "installCRDs"
    value = "true"
  }

  set {
    name = "replicaCount"
    value = "2"
  }

  # https://cert-manager.io/docs/configuration/acme/dns01/#setting-nameservers-for-dns01-self-check
  set {
    name = "extraArgs"
    value = "{--dns01-recursive-nameservers-only,--dns01-recursive-nameservers=1.1.1.1:53\\,8.8.8.8:53}"
  }

  set {
    name = "prometheus.servicemonitor.enabled"
    value = "true"
  }

  set {
    name = "prometheus.servicemonitor.prometheusInstance"
    value = "qovery"
  }

  # Limits
  set {
    name = "resources.limits.cpu"
    value = "200m"
  }

  set {
    name = "resources.requests.cpu"
    value = "100m"
  }

  set {
    name = "resources.limits.memory"
    value = "1Gi"
  }

  set {
    name = "resources.requests.memory"
    value = "1Gi"
  }

  # Limits webhook
  set {
    name = "webhook.resources.limits.cpu"
    value = "20m"
  }

  set {
    name = "webhook.resources.requests.cpu"
    value = "20m"
  }

  set {
    name = "webhook.resources.limits.memory"
    value = "128Mi"
  }

  set {
    name = "webhook.resources.requests.memory"
    value = "128Mi"
  }

  # Limits cainjector
  set {
    name = "cainjector.resources.limits.cpu"
    value = "500m"
  }

  set {
    name = "cainjector.resources.requests.cpu"
    value = "100m"
  }

  set {
    name = "cainjector.resources.limits.memory"
    value = "1Gi"
  }

  set {
    name = "cainjector.resources.requests.memory"
    value = "1Gi"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.cluster_autoscaler,
    helm_release.aws_vpc_cni,
  ]
}

resource "helm_release" "cert_manager_config" {
  name = "cert-manager-configs"
  chart = "common/charts/cert-manager-configs"
  namespace = "cert-manager"
  atomic = true
  max_history = 50

  depends_on = [helm_release.cert_manager]

  set {
    name = "externalDnsProvider"
    value = "{{ external_dns_provider }}"
  }

  set {
    name = "acme.letsEncrypt.emailReport"
    value = "{{ dns_email_report }}"
  }

  set {
    name = "acme.letsEncrypt.acmeUrl"
    value = "{{ acme_server_url }}"
  }

  set {
    name = "managedDns"
    value = "{{ managed_dns_domains_terraform_format }}"
  }

{% if external_dns_provider == "cloudflare" %}
  set {
    name = "provider.cloudflare.apiToken"
    value = "{{ cloudflare_api_token }}"
  }

  set {
    name = "provider.cloudflare.email"
    value = "{{ cloudflare_email }}"
  }
{% endif %}
}
