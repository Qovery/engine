#!/bin/bash
set -e

ROOT_MODULE_PATH=$1
CMD=$2
PLAN_NAME=$3
shift 3

mkdir -p /persistent-volume/terraform-work
mkdir -p /persistent-volume/terraform-plan-output

rsync -a --delete \
          --exclude='entrypoint.sh' \
          --exclude='Dockerfile.qovery' \
          --exclude='.terraform' \
          --exclude='.terraform.lock.hcl' \
          --exclude='.-tf.plan' \
          /data/ /persistent-volume/terraform-work

cd /persistent-volume/terraform-work/"$ROOT_MODULE_PATH"

log() {
  echo -e "\n[==> TERRAFORM]: $1\n"
}


run_terraform_init() {
  log "terraform init $TF_CLI_ARGS_init"
  terraform init -backend-config="/backend-config/config" 2>&1 \
    | awk '{print} /Terraform has been successfully initialized!/ {exit}'
}

attempt_force_unlock() {
  # Try to detect if state is locked by attempting a plan operation
  LOCK_OUTPUT=$(terraform plan -input=false 2>&1 || true)
  # Extract lock ID from the error message
  LOCK_ID=$(echo "$LOCK_OUTPUT" | grep -oE 'ID:[[:space:]]*[0-9a-fA-F-]{36}' | sed 's/ID:[[:space:]]*//' | head -1)
  if [ -n "$LOCK_ID" ]; then
    log "found lock ID: $LOCK_ID"
    log "terraform force-unlock -force $LOCK_ID"
    terraform force-unlock -force "$LOCK_ID" || true
  else
    log "could not extract lock ID"
  fi
}

case "$CMD" in
    "init")
        run_terraform_init
        ;;
    "apply")
        run_terraform_init
        log "terraform validate $TF_CLI_ARGS_validate"
        terraform validate
        log "terraform apply -input=false -auto-approve"
        terraform apply -input=false -auto-approve "$@"
        log "terraform output"
        terraform output -json > /qovery-output/qovery-output.json
        ;;
    "plan_only")
        run_terraform_init
        log "terraform validate $TF_CLI_ARGS_validate"
        terraform validate
        log "terraform plan $TF_CLI_ARGS_plan"
        terraform plan -input=false -out=/persistent-volume/terraform-plan-output/"${PLAN_NAME}"-tf.plan "$@"
        ;;
    "apply_from_plan")
        run_terraform_init
        log "terraform validate $TF_CLI_ARGS_validate"
        terraform validate
        log "terraform apply $TF_CLI_ARGS_apply"
        terraform apply -input=false /persistent-volume/terraform-plan-output/"${PLAN_NAME}"-tf.plan
        log "terraform output $TF_CLI_ARGS_output"
        terraform output -json > /qovery-output/qovery-output.json
        ;;
    "destroy")
        log "terraform destroy $TF_CLI_ARGS_destroy"
        terraform destroy -auto-approve -input=false "$@"
        ;;
    "unlock_state")
        run_terraform_init
        attempt_force_unlock
        ;;
    *)
        echo "Command not handled by entrypoint.sh: '\$CMD'"
        exit 1
        ;;
esac
