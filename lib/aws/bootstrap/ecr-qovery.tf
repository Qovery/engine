# Qovery registry repository for application images store
data "external" "ecr_qovery_repo" {
  program = ["./helper.sh", "create_ecr_repository", "qovery"]
}