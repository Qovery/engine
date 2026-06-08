terraform {
  required_providers {
    qovery = {
      source = "qovery/qovery"
    }
  }
}

provider "qovery" {}

resource "qovery_helm_repository" "blueprint_repo" {
  organization_id       = "{{ organization_id | hcl_string }}"
  name                  = "blueprint-{{ service_name | hcl_string }}-{{ execution_id | hcl_string }}"
  kind                  = "{{ chart_repository_kind }}"
  url                   = "{{ chart_repository | hcl_string }}"
  skip_tls_verification = false
}

resource "qovery_helm" "blueprint" {
  environment_id               = "{{ environment_id | hcl_string }}"
  blueprint_id                 = "{{ blueprint_id | hcl_string }}"
  name                         = "{{ name | hcl_string }}"
  description                  = "{{ description | hcl_string }}"
  allow_cluster_wide_resources = {{ allow_cluster_wide_resources }}
  auto_deploy                  = false
{% if timeout_sec %}
  timeout_sec                  = {{ timeout_sec }}
{% endif %}

  source = {
    helm_repository = {
      helm_repository_id = qovery_helm_repository.blueprint_repo.id
      chart_name         = "{{ chart_name | hcl_string }}"
      chart_version      = "{{ chart_version | hcl_string }}"
    }
  }

  values_override = {
{% if rendered_values %}
    file = {
      raw = {
        "blueprint-values" = {
          content = <<-EOT
{{ rendered_values | hcl_heredoc }}
          EOT
        }
      }
    }
{% endif %}
  }

{% if arguments | length > 0 %}
  arguments = [
{% for arg in arguments %}
    "{{ arg | hcl_string }}",
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
  id = "{{ import_id | hcl_string }}"
}
{% endif %}
