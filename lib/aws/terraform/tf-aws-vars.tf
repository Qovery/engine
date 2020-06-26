provider "aws" {
  profile    = "default"
  access_key = ""
  secret_key = ""
  region     = var.region
  version    = "~> 2.63"
}

provider "local" {
  version = "~> 1.4"
}
