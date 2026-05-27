terraform {
  required_providers {
    qovery = {
      source = "qovery/qovery"
    }
  }
}

provider "qovery" {}

resource "qovery_terraform_service" "blueprint" {
  environment_id        = "{{ environment_id }}"
  name                  = "{{ name }}"
  auto_deploy           = true
  engine                = "{{ engine }}"
  timeout_seconds       = {{ timeout_seconds }}
  use_cluster_credentials = {{ use_cluster_credentials }}

  git_repository = {
    url       = "{{ git_url }}"
    branch    = "{{ git_branch }}"
    root_path = "{{ git_root_path }}"
{% if git_token_id %}
    git_token_id = "{{ git_token_id }}"
{% endif %}
  }

  backend = {
{% if backend_kubernetes %}
    kubernetes = {}
{% elif backend_blueprint %}
    blueprint = {
      type = "{{ backend_type }}"
{% if backend_config %}
      config = {
{% for key, value in backend_config %}
        {{ key }} = "{{ value }}"
{% endfor %}
      }
{% endif %}
    }
{% else %}
    user_provided = {}
{% endif %}
  }

  engine_version = {
    explicit_version          = "{{ engine_version }}"
    read_from_terraform_block = false
  }

  job_resources = {
    cpu_milli   = {{ job_cpu_milli }}
    ram_mib     = {{ job_ram_mib }}
    storage_gib = {{ job_storage_gib }}
  }

  tfvars_files = []

  variables = [
{% for var in variables %}
    {
      key       = "TF_VAR_{{ var.name }}"
      value     = "{{ var.value }}"
      is_secret = {{ var.is_secret }}
    },
{% endfor %}
  ]
}

{% if import_id %}
import {
  to = qovery_terraform_service.blueprint
  id = "{{ import_id }}"
}
{% endif %}
