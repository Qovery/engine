terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version    = "~> 4.12.1"
    }
    external = {
      source = "hashicorp/external"
      version = "~> 2.2"
    }
    vault = {
      source = "hashicorp/vault"
      version = "~> 3.5"
    }
    local = {
      source = "hashicorp/local"
      version = "~> 2.2"
    }
    null = {
      source = "hashicorp/null"
      version = "~> 3.1"
    }
    random = {
      source = "hashicorp/random"
      version = "~> 3.1"
    }
    time = {
      source  = "hashicorp/time"
      version = "~> 0.7"
    }
  }
  required_version = ">= 0.13"
}

provider "aws" {
  profile    = "default"
  access_key = "{{ aws_access_key }}"
  secret_key = "{{ aws_secret_key }}"
  region     = "{{ aws_region }}"
}

provider "aws" {
  alias = "tfstates"
  access_key = "{{ aws_access_key_tfstates_account }}"
  secret_key = "{{ aws_secret_key_tfstates_account }}"
  region = "{{ aws_region_tfstates_account }}"
}

provider "vault" {
  {% if vault_auth_method == "app_role" and not test_cluster %}
  auth_login {
    path = "auth/approle/login"

    parameters = {
      role_id   = "{{ vault_role_id }}"
      secret_id = "{{ vault_secret_id }}"
    }
  }
  {% endif %}
}
