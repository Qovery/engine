terraform {
  required_providers {
    qovery = {
      source = "qovery/qovery"
    }
  }
}

provider "qovery" {}

resource "qovery_helm_repository" "blueprint_repo" {
  organization_id       = "{{ organization_id }}"
  name                  = "blueprint-{{ service_name }}-{{ execution_id }}"
  kind                  = "HTTPS"
  url                   = "{{ chart_repository }}"
  skip_tls_verification = false
}

resource "qovery_helm" "blueprint" {
  environment_id               = "{{ environment_id }}"
  name                         = "{{ name }}"
  description                  = "Deployed from blueprint"
  allow_cluster_wide_resources = {{ allow_cluster_wide_resources }}
  auto_deploy                  = true
{% if timeout_sec %}
  timeout_sec                  = {{ timeout_sec }}
{% endif %}

  source = {
    helm_repository = {
      helm_repository_id = qovery_helm_repository.blueprint_repo.id
      chart_name         = "{{ chart_name }}"
      chart_version      = "{{ chart_version }}"
    }
  }

  values_override = {
{% if rendered_values %}
    file = {
      raw = {
        "blueprint-values" = {
          content = <<-EOT
{{ rendered_values }}
          EOT
        }
      }
    }
{% endif %}
  }

{% if arguments | length > 0 %}
  arguments = [
{% for arg in arguments %}
    "{{ arg }}",
{% endfor %}
  ]
{% endif %}

  depends_on = [
    qovery_helm_repository.blueprint_repo,
  ]
}

{% if import_id %}
import {
  to = qovery_helm.blueprint
  id = "{{ import_id }}"
}
{% endif %}
