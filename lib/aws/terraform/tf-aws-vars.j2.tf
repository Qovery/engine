provider "aws" {
  profile    = "default"
  access_key = "{{ aws_access_key }}"
  secret_key = "{{ aws_secret_key }}"
  region     = "{{ aws_region }}"
  version    = "~> 2.63"
}

provider "local" {
  version = "~> 1.4"
}
