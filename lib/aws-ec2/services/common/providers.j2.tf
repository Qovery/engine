terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version    = "~> 3.36.0"
    }
    local = {
      source = "hashicorp/local"
      version = "~> 1.4"
    }
    time = {
      source  = "hashicorp/time"
      version = "~> 0.3"
    }
  }
  required_version = ">= 0.14"
}

provider "aws" {
  profile    = "default"
  region     = "{{ region }}"
  access_key = "{{ aws_access_key }}"
  secret_key = "{{ aws_secret_key }}"
}

resource "time_static" "on_db_create" {}
