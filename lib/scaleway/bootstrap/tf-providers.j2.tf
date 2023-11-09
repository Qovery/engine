provider "aws" {
  alias = "tfstates"
  access_key = "{{ aws_access_key_tfstates_account }}"
  secret_key = "{{ aws_secret_key_tfstates_account }}"
  region = "{{ aws_region_tfstates_account }}"
}

terraform {
  required_providers {
    scaleway = {
      source = "scaleway/scaleway"
      version = "2.31.0"
    }
    aws = {
      source = "hashicorp/aws"
      version    = "4.51.0"
    }
    local = {
      source = "hashicorp/local"
      version = "2.2.3"
    }
    time = {
      source  = "hashicorp/time"
      version = "0.9.0"
    }
  }
  required_version = "1.3.3"
}


provider "scaleway" {
  access_key      = var.scaleway_access_key
  secret_key      = var.scaleway_secret_key
  project_id	  = var.scaleway_project_id
  zone            = var.zone
  region          = var.region
}