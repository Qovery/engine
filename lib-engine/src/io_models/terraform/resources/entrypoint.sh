#!/bin/bash

set -eo pipefail

TF_COMMAND={{terraform_command}}
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
  printf "\n[==> %s]: %s\n" "${TF_COMMAND}" "$1"
}

extract_terraform_resources() {
  log "${TF_COMMAND} show -json (extracting resources)"
  # Extract terraform resources and save to output directory
  # Non-blocking: if this fails, deployment still succeeds
  if ${TF_COMMAND} show -json > /qovery-output/terraform-resources.json 2>/dev/null; then
    log "Successfully extracted terraform resources"
  else
    log "Warning: Failed to extract terraform resources (non-fatal)"
    # Create empty resources file so processing continues
    echo '{"resources": []}' > /qovery-output/terraform-resources.json
  fi
}


run_terraform_init() {
  log "${TF_COMMAND} init $TF_CLI_ARGS_init"
  # We want to check for this specific patterns to avoid terraform returning success if it inits on an empty dir
  ${TF_COMMAND} init -backend-config="/backend-config/config" 2>&1 \
    | awk '{print} /has been successfully initialized!/ {found=1} END {if (found) exit 0; else exit 1}'
}

attempt_force_unlock() {
  # Try to detect if state is locked by attempting a plan operation
  LOCK_OUTPUT=$(${TF_COMMAND} plan -input=false 2>&1 || true)
  # Extract lock ID from the error message
  LOCK_ID=$(echo "$LOCK_OUTPUT" | grep -oE 'ID:[[:space:]]*[0-9a-fA-F-]{36}' | sed 's/ID:[[:space:]]*//' | head -1)
  if [ -n "$LOCK_ID" ]; then
    log "found lock ID: $LOCK_ID"
    log "${TF_COMMAND} force-unlock -force $LOCK_ID"
    ${TF_COMMAND} force-unlock -force "$LOCK_ID" || true
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
        log "${TF_COMMAND} validate $TF_CLI_ARGS_validate"
        ${TF_COMMAND} validate
        log "${TF_COMMAND} apply -input=false -auto-approve"
        ${TF_COMMAND} apply -input=false -auto-approve "$@"
        log "${TF_COMMAND} output"
        ${TF_COMMAND} output -json > /qovery-output/qovery-output.json
        extract_terraform_resources
        ;;
    "plan_only")
        run_terraform_init
        log "${TF_COMMAND} validate $TF_CLI_ARGS_validate"
        ${TF_COMMAND} validate
        log "${TF_COMMAND} plan $TF_CLI_ARGS_plan"
        ${TF_COMMAND} plan -input=false -out=/persistent-volume/terraform-plan-output/"${PLAN_NAME}"-tf.plan "$@"
        ;;
    "apply_from_plan")
        run_terraform_init
        log "${TF_COMMAND} validate $TF_CLI_ARGS_validate"
        ${TF_COMMAND} validate
        log "${TF_COMMAND} apply $TF_CLI_ARGS_apply"
        ${TF_COMMAND} apply -input=false /persistent-volume/terraform-plan-output/"${PLAN_NAME}"-tf.plan
        log "${TF_COMMAND} output $TF_CLI_ARGS_output"
        ${TF_COMMAND} output -json > /qovery-output/qovery-output.json
        extract_terraform_resources
        ;;
    "destroy")
        log "${TF_COMMAND} destroy $TF_CLI_ARGS_destroy"
        ${TF_COMMAND} destroy -auto-approve -input=false "$@"
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
