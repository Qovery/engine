resource "helm_release" "cert_manager" {
  name = "cert-manager"
  chart = "common/charts/cert-manager"
  namespace = "cert-manager"
  create_namespace = true
  atomic = true
  max_history = 50

  values = [file("chart_values/cert-manager.yaml")]

  set {
    name = "version"
    value = "0.16.0"
  }

  set {
    name = "installCRDs"
    value = "true"
  }

  set {
    name = "replicaCount"
    value = "2"
  }

  set {
    name = "podDnsPolicy"
    value = "None"
  }

  set {
    name = "podDnsConfig.nameservers"
    value = "{1.1.1.1,8.8.8.8}"
  }

  set {
    name = "prometheus.servicemonitor.enabled"
    value = "true"
  }

  set {
    name = "prometheus.servicemonitor.prometheusInstance"
    value = "qovery"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
    helm_release.prometheus_operator
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
    name = "emailReport"
    value = "{{ dns_email_report }}" // Todo: customize it with client address?
  }

  set {
    name = "managedDns"
    value = "{{ managed_dns_terraform_format }}"
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