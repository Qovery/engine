terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version    = "4.34.0"
    }
    external = {
      source = "hashicorp/external"
      version = "2.2.2"
    }
    local = {
      source = "hashicorp/local"
      version = "2.2.3"
    }
    null = {
      source = "hashicorp/null"
      version = "3.1.1"
    }
    random = {
      source = "hashicorp/random"
      version = "3.4.3"
    }
    time = {
      source  = "hashicorp/time"
      version = "0.9.0"
    }
  }
  required_version = "1.3.3"
}

provider "aws" {
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
