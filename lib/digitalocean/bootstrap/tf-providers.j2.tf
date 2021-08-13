provider "aws" {
  alias = "tfstates"
  access_key = "{{ aws_access_key_tfstates_account }}"
  secret_key = "{{ aws_secret_key_tfstates_account }}"
  region = "{{ aws_region_tfstates_account }}"
}

provider "digitalocean" {
  token = "{{ digitalocean_token }}"
  spaces_access_id = "{{ spaces_access_id }}"
  spaces_secret_key = "{{ spaces_secret_key }}"
}

terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version    = "~> 3.36.0"
    }
    digitalocean = {
      source = "digitalocean/digitalocean"
      version = "~> 2.11.0"
    }
    external = {
      source = "hashicorp/external"
      version = "~> 1.2"
    }
    local = {
      source = "hashicorp/local"
      version = "~> 1.4"
    }
    null = {
      source = "hashicorp/null"
      version = "~> 2.1"
    }
    random = {
      source = "hashicorp/random"
      version = "~> 2.3"
    }
  }
  required_version = ">= 0.14"
}