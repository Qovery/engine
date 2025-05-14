terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version    = "5.84.0"
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
  required_version = "1.9.7"
}

provider "aws" {
  region     = "{{ aws_region }}"
  access_key = "{{ aws_access_key }}"
  secret_key = "{{ aws_secret_key }}"
{% if aws_session_token -%}
  token = "{{ aws_session_token }}"
{% endif -%}
}