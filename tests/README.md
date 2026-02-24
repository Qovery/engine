# Tests

Several kind of tests exists in the Engine:
* Unit tests: offer simple tests on some parts of the system
* Functional tests: they are used to ensure the finality of a deployment/update/delete

Of course functional tests takes a longer time to deploy and they requests several specific access to be used (ex: AWS credentials for AWS cloud provider testing)

# Intellij Idea Configuration

The option "Use all features in test" must be enabled.

# Usage

In order to perform functional tests, you must run the following command in order.
1. `mise get-test-secrets`: This will export your secret in `/tmp/engine_test_secrets.env` and will be loaded in `FuncTestsSecrets::new()`
2. `mise your-test-suite`

### TTL
By default, all deployed tests resources are going to be tagged with a TTL, to be automatically cleaned with [Pleco](https://github.com/Qovery/pleco) if a test fail for some reasons.

This ttl is set by default to 1h, but you can override it with a `ttl` environment variable in seconds like: `ttl=7200`.

### Terraform dry run
If you just want to render Terraform without applying changes, you can set `dry_run_deploy` environment variable to anything to enable it like `dry_run_deploy=true`.

### Custom cluster id
It can be useful sometimes to be able to add a custom cluster id during tests. In order to do that, simply use `custom_cluster_id` environment variable with the desired name.

Note: remind that you can't need to use valid chars https://datatracker.ietf.org/doc/html/rfc8117

### Random cluster id
To enable the generation of random cluster name when testing cluster creation, we can define the `CI_PROJECT_TITLE` with any value (see `generate_cluster_id` method in `utilities.rs`).

### Forced upgrade
By default, helm charts are applied only when they do not exist or when they receive an update.

During chart upgrade or atomic rollback, Terraform is not able to catch those changes and requires an upgrade.
In order to perform it, you need the variable `forced_upgrade` to `true` to ensure everything is up to date.

The advantage of having it set to `false` by default, is the deployment speed. Only helm changes are going to be applied. The drawback is you can't
be 100% sure of what you've deployed is what you asked for on your infra.

