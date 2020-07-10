# Qovery registry repository for application images store
data "external" "ecr-qovery-repo" {
  program = ["./helper.sh", "create_ecr_repository", "qovery"]
}