provider "aws" {
  profile    = "default"
  region     = "{{ region }}"
  version    = "~> 2.63"
}

provider "local" {
  version = "~> 1.4"
}
