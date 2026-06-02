terraform {
  required_providers {
    qovery = {
      source = "qovery/qovery"
    }
  }
}

provider "qovery" {}

resource "qovery_terraform_service" "blueprint" {
  environment_id        = "{{ environment_id | hcl_string }}"
  name                  = "{{ name | hcl_string }}"
  auto_deploy           = false
  engine                = "{{ engine | hcl_string }}"
  timeout_seconds       = {{ timeout_seconds }}
  use_cluster_credentials = {{ use_cluster_credentials }}

  git_repository = {
    url       = "{{ git_url | hcl_string }}"
    branch    = "{{ git_branch | hcl_string }}"
    root_path = "{{ git_root_path | hcl_string }}"
{% if git_token_id %}
    git_token_id = "{{ git_token_id | hcl_string }}"
{% endif %}
  }

  backend = {
{% if backend_kubernetes %}
    kubernetes = {}
{% elif backend_blueprint %}
    blueprint = {
      type = "{{ backend_type | hcl_string }}"
{% if backend_config %}
      config = {
{% for key, value in backend_config %}
        {{ key }} = "{{ value | hcl_string }}"
{% endfor %}
      }
{% endif %}
    }
{% else %}
    user_provided = {}
{% endif %}
  }

  engine_version = {
    explicit_version          = "{{ engine_version | hcl_string }}"
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
      key       = "TF_VAR_{{ var.name | hcl_string }}"
      value     = "{{ var.value | hcl_string }}"
      is_secret = {{ var.is_secret }}
    },
{% endfor %}
  ]
}

{% if import_id %}
import {
  to = qovery_terraform_service.blueprint
  id = "{{ import_id | hcl_string }}"
}
{% endif %}
