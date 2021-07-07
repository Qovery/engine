terraform {
  required_providers {
    scaleway = {
      source = "scaleway/scaleway"
    }
    local = {
      source = "hashicorp/local"
      version = "~> 1.4"
    }
    time = {
      source  = "hashicorp/time"
      version = "~> 0.3"
    }
    vault = {
      source = "hashicorp/vault"
      version = "~> 2.18.0"
    }
  }
  required_version = ">= 0.13"
}


provider "scaleway" {
  access_key      = var.scaleway_access_key
  secret_key      = var.scaleway_secret_key
  project_id	  = var.scaleway_default_project_id
  zone            = var.scaleway_default_zone
  region          = var.region
}

provider "kubernetes" {
  load_config_file = "false"

  host  = null_resource.kubeconfig.triggers.host
  token = null_resource.kubeconfig.triggers.token
  cluster_ca_certificate = base64decode(
    null_resource.kubeconfig.triggers.cluster_ca_certificate
  )
}

provider "vault" {
  # TODO(benjaminch): un-comment and let jinja template to process it
  #{% if vault_auth_method == "app_role" and not test_cluster %}
  #auth_login {
  #  path = "auth/approle/login"
  #
  #  parameters = {
  #    role_id   = "{{ vault_role_id }}"
  #    secret_id = "{{ vault_secret_id }}"
  #  }
  #}
  #{% endif %}
}
