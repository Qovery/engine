terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version = "6.33.0"
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

{% if aws_wide_permissions_enabled -%}
# The default provider uses the wide permissions credentials, this one uses the cloud provider options credentials
# so we can resolve their identity and keep their access to the cluster
provider "aws" {
  alias      = "wide_permissions_options_credentials"
  region     = "{{ aws_region }}"
  access_key = "{{ aws_wide_permissions_options_access_key }}"
  secret_key = "{{ aws_wide_permissions_options_secret_key }}"
  token      = "{{ aws_wide_permissions_options_session_token }}"
}
{%- endif %}