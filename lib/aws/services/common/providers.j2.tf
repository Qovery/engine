provider "aws" {
  profile    = "default"
  region     = "{{ qovery_env.region }}"
  version    = "~> 2.63"
}

provider "local" {
  version = "~> 1.4"
}
