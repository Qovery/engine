terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version    = "~> 3.66.0"
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
    time = {
      source  = "hashicorp/time"
      version = "~> 0.3"
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
