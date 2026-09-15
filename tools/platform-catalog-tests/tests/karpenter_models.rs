//! Karpenter models evaluated through the same isolated entrypoint as q-core,
//! then rendered with the actual vendored Helm charts. No AWS or Kubernetes access.

use platform_catalog_tests::{
    assert_success, document_by_kind_and_name, helm_template, parse_yaml_documents, repository_path, yaml_path,
};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

const CONFIG_CHART: &str = "lib-engine/lib/aws/bootstrap/charts/karpenter-custom-resources";
const CONTROLLER_CHART: &str = "lib-engine/lib/aws/bootstrap/charts/karpenter";

fn request(example: &str) -> Value {
    serde_json::from_slice(
        &fs::read(repository_path(format!(
            "platform-catalog/examples/karpenter-v0/{example}.request.json"
        )))
        .unwrap(),
    )
    .unwrap()
}

fn copy_bundle(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.path().is_dir() {
            copy_bundle(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}

fn evaluate(request: &Value) -> Value {
    let component = request["componentKey"].as_str().unwrap();
    let isolated = tempdir().unwrap();
    copy_bundle(
        &repository_path(format!("platform-catalog/components/{component}/config/runtime-values")),
        isolated.path(),
    );
    let output = assert_success(
        Command::new(std::env::var_os("PKL_BIN").unwrap_or_else(|| "pkl".into()))
            .arg("eval")
            .arg("--root-dir")
            .arg(isolated.path())
            .arg("--allowed-modules=^pkl:,^file:")
            .arg("--allowed-resources=^prop:request$")
            .arg("--no-cache")
            .arg("-p")
            .arg(format!("request={request}"))
            .arg(isolated.path().join("model.pkl")),
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn compiled(request: &Value) -> Value {
    let result = evaluate(request);
    assert_eq!(result["violations"], json!([]), "{result}");
    result
        .get("helmValues")
        .expect("valid compilation must produce values")
        .clone()
}

fn rendered_configuration(request: &Value) -> Vec<Value> {
    let directory = tempdir().unwrap();
    let values = directory.path().join("values.yaml");
    fs::write(&values, serde_yaml::to_string(&compiled(request)).unwrap()).unwrap();
    parse_yaml_documents(&helm_template(
        "karpenter-configuration",
        CONFIG_CHART,
        "kube-system",
        &[values],
        &[],
    ))
    .into_iter()
    .map(|value| serde_json::to_value(value).unwrap())
    .collect()
}

fn resource<'a>(documents: &'a [Value], kind: &str, name: &str) -> &'a Value {
    documents
        .iter()
        .find(|doc| doc["kind"] == kind && doc["metadata"]["name"] == name)
        .unwrap()
}

#[test]
fn four_operations_preserve_the_sdk_envelope_and_compile_only_valid_complete_requests() {
    for example in ["configuration", "controller"] {
        for operation in ["DESCRIBE", "RESOLVE_REQUIREMENTS", "VALIDATE", "COMPILE"] {
            let mut req = request(example);
            req["operation"] = json!(operation);
            if matches!(operation, "DESCRIBE" | "RESOLVE_REQUIREMENTS") {
                req["clusterInputs"] = json!({});
            }
            if operation == "DESCRIBE" {
                req["clusterContext"] = Value::Null;
            }
            let result = evaluate(&req);
            assert_eq!(result["violations"], json!([]), "{example}/{operation}: {result}");
            assert_eq!(result.get("helmValues").is_some(), operation == "COMPILE");
            assert_eq!(
                result["requiredInputs"].as_array().unwrap().len(),
                if operation == "DESCRIBE" { 0 } else { 3 }
            );
            assert!(!result["fields"].as_array().unwrap().is_empty());
        }
    }
}

#[test]
fn custom_subnet_count_matches_the_crd_limit_in_validation_compilation_and_descriptors() {
    let crds: Vec<Value> = parse_yaml_documents(&helm_template(
        "karpenter-crd",
        "lib-engine/lib/aws/bootstrap/charts/karpenter-crd",
        "kube-system",
        &[],
        &[],
    ))
    .into_iter()
    .map(|value| serde_json::to_value(value).unwrap())
    .collect();
    let crd = resource(&crds, "CustomResourceDefinition", "ec2nodeclasses.karpenter.k8s.aws");
    let version = crd["spec"]["versions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|version| version["name"] == "v1")
        .unwrap();
    assert_eq!(
        version["schema"]["openAPIV3Schema"]["properties"]["spec"]["properties"]["subnetSelectorTerms"]["maxItems"],
        30
    );

    for count in [30, 31] {
        let mut req = request("configuration");
        req["profileConfig"]["nodePools"][1]["subnets"]["ids"] = json!(
            (0..count)
                .map(|index| format!("subnet-{index:017x}"))
                .collect::<Vec<_>>()
        );
        for operation in ["VALIDATE", "COMPILE"] {
            req["operation"] = json!(operation);
            let result = evaluate(&req);
            if count == 30 {
                assert_eq!(result["violations"], json!([]), "{operation}: {result}");
                assert_eq!(result.get("helmValues").is_some(), operation == "COMPILE");
                if operation == "COMPILE" {
                    let docs = rendered_configuration(&req);
                    assert_eq!(
                        resource(&docs, "EC2NodeClass", "qovery-isolated")["spec"]["subnetSelectorTerms"]
                            .as_array()
                            .unwrap()
                            .len(),
                        count
                    );
                }
            } else {
                let violations = result["violations"].as_array().unwrap();
                assert_eq!(violations.len(), 1, "{operation}: {result}");
                assert_eq!(violations[0]["code"], "LENGTH_OUT_OF_RANGE");
                assert_eq!(violations[0]["fieldPath"], "nodePools[1].subnets.ids");
                assert!(result.get("helmValues").is_none(), "{operation}: {result}");
            }
        }
        req["operation"] = json!("DESCRIBE");
        let described = evaluate(&req);
        let subnets = described["fields"][0]["itemFields"][1]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["key"] == "subnets")
            .unwrap();
        let ids = subnets["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["key"] == "ids")
            .unwrap();
        assert_eq!(ids["constraints"]["maxItems"], 30);
    }
}

#[test]
fn two_pools_render_as_independent_pairs_with_legacy_defaults_and_stable_names() {
    let mut req = request("configuration");
    let docs = rendered_configuration(&req);
    assert_eq!(docs.len(), 4, "no legacy stable/default/GPU pool may be added");
    let general = resource(&docs, "NodePool", "qovery-general");
    let isolated = resource(&docs, "NodePool", "qovery-isolated");
    let general_class = resource(&docs, "EC2NodeClass", "qovery-general");
    let isolated_class = resource(&docs, "EC2NodeClass", "qovery-isolated");
    for (pool, name) in [(general, "qovery-general"), (isolated, "qovery-isolated")] {
        assert_eq!(
            pool["spec"]["template"]["spec"]["nodeClassRef"],
            json!({"group":"karpenter.k8s.aws", "kind":"EC2NodeClass", "name":name})
        );
        assert_eq!(pool["spec"]["template"]["spec"]["expireAfter"], "720h");
        assert_eq!(pool["spec"]["template"]["spec"]["terminationGracePeriod"], "4h");
        assert_eq!(pool["spec"]["disruption"]["consolidateAfter"], "1m");
        assert_eq!(pool["spec"]["disruption"]["budgets"], json!([{"nodes":"10%"}]));
        assert_eq!(pool["spec"]["weight"], 50);
    }
    assert_eq!(
        general["spec"]["template"]["spec"]["requirements"],
        json!([
            {"key":"node.kubernetes.io/instance-type","operator":"In","values":["c7i-flex.large","c7i.metal-24xl"]},
            {"key":"karpenter.sh/capacity-type","operator":"In","values":["spot","on-demand"]}
        ])
    );
    assert_eq!(
        isolated["spec"]["template"]["spec"]["requirements"][1]["values"],
        json!(["on-demand"])
    );
    assert_eq!(general["spec"]["disruption"]["consolidationPolicy"], "WhenEmptyOrUnderutilized");
    assert_eq!(isolated["spec"]["disruption"]["consolidationPolicy"], "WhenEmpty");
    assert_eq!(
        isolated["spec"]["template"]["spec"]["taints"],
        req["profileConfig"]["nodePools"][1]["taints"]
    );
    assert_eq!(
        general_class["spec"],
        json!({
            "role":"customer-node-role", "metadataOptions":{"httpPutResponseHopLimit":2},
            "securityGroupSelectorTerms":[{"id":"sg-0123456789abcdef0"}],
            "amiSelectorTerms":[{"alias":"al2023@latest"}],
            "subnetSelectorTerms":[{"tags":{"karpenter.sh/discovery":"customer-eks"}}]
        })
    );
    assert_eq!(isolated_class["spec"]["amiFamily"], "AL2023");
    assert_eq!(
        isolated_class["spec"]["amiSelectorTerms"],
        json!([{"id":"ami-00000000000000000"}])
    );
    assert_eq!(
        isolated_class["spec"]["subnetSelectorTerms"],
        json!([{"id":"subnet-00000000000000000"}])
    );
    req["profileConfig"]["nodePools"].as_array_mut().unwrap().reverse();
    let reordered = rendered_configuration(&req);
    for original in &docs {
        assert_eq!(
            resource(
                &reordered,
                original["kind"].as_str().unwrap(),
                original["metadata"]["name"].as_str().unwrap()
            ),
            original
        );
    }
}

#[test]
fn descriptors_follow_each_active_row_and_discovery_defaults_use_the_actual_eks_name() {
    let mut req = request("configuration");
    req["operation"] = json!("DESCRIBE");
    let result = evaluate(&req);
    let rows = result["fields"][0]["itemFields"].as_array().unwrap();
    let field = |row: usize, key: &str| {
        rows[row]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["key"] == key)
            .unwrap()
    };
    assert_eq!(field(0, "ami")["fields"].as_array().unwrap().len(), 1);
    assert_eq!(field(1, "ami")["fields"][1]["key"], "id");
    assert_eq!(field(0, "subnets")["fields"][1]["fields"][1]["defaultValue"], "customer-eks");
    assert_eq!(field(1, "subnets")["fields"][1]["key"], "ids");
    req["clusterInputs"] = json!({});
    assert!(evaluate(&req)["fields"][0]["items"]["fields"][6]["fields"][1]["fields"][1]["defaultValue"].is_null());
}

#[test]
fn controller_render_keeps_upstream_resilience_and_pins_an_independent_ec2_group() {
    let req = request("controller");
    let directory = tempdir().unwrap();
    let values = directory.path().join("values.yaml");
    fs::write(&values, serde_yaml::to_string(&compiled(&req)).unwrap()).unwrap();
    let docs = parse_yaml_documents(&helm_template(
        "karpenter",
        CONTROLLER_CHART,
        "kube-system",
        &[
            repository_path("platform-catalog/components/karpenter/config/static-values/base.yaml"),
            values,
        ],
        &[],
    ));
    let deployment = document_by_kind_and_name(&docs, "Deployment", "karpenter").unwrap();
    let deployment: Value = serde_json::to_value(deployment).unwrap();
    let pod = &deployment["spec"]["template"]["spec"];
    assert_eq!(deployment["spec"]["replicas"], 2);
    assert_eq!(
        pod["containers"][0]["resources"],
        json!({"requests": {"cpu": "100m", "memory": "1Gi"}, "limits": {"memory": "1Gi"}})
    );
    assert_eq!(
        pod["nodeSelector"],
        json!({"kubernetes.io/os":"linux","eks.amazonaws.com/nodegroup":"customer-system"})
    );
    assert_eq!(
        pod["affinity"]["nodeAffinity"]["requiredDuringSchedulingIgnoredDuringExecution"]["nodeSelectorTerms"],
        json!([
            {"matchExpressions":[{"key":"karpenter.sh/nodepool","operator":"DoesNotExist"},{"key":"eks.amazonaws.com/compute-type","operator":"NotIn","values":["fargate"]}]}
        ])
    );
    assert_eq!(
        pod["affinity"]["podAntiAffinity"]["requiredDuringSchedulingIgnoredDuringExecution"][0]["topologyKey"],
        "kubernetes.io/hostname"
    );
    assert_eq!(
        pod["topologySpreadConstraints"][0]["topologyKey"],
        "topology.kubernetes.io/zone"
    );
    assert_eq!(
        pod["tolerations"],
        json!([
            {"key":"CriticalAddonsOnly","operator":"Exists"},
            {"key":"dedicated","value":"","operator":"Equal","effect":"NoSchedule"}
        ])
    );
    let env = pod["containers"][0]["env"].as_array().unwrap();
    assert!(
        env.iter()
            .any(|entry| entry["name"] == "CLUSTER_NAME" && entry["value"] == "customer-eks")
    );
    assert!(
        env.iter()
            .any(|entry| entry["name"] == "INTERRUPTION_QUEUE" && entry["value"] == "customer-karpenter-interruptions")
    );
    let account = document_by_kind_and_name(&docs, "ServiceAccount", "karpenter").unwrap();
    assert_eq!(
        yaml_path(account, &["metadata", "annotations", "eks.amazonaws.com/role-arn"])
            .unwrap()
            .as_str()
            .unwrap(),
        "arn:aws:iam::123456789012:role/karpenter/controller"
    );
}

#[test]
fn customer_tag_values_remain_literal_data_through_both_rendering_stages() {
    let mut req = request("configuration");
    let key = "customer/tag: \"réseau\"";
    let value = "{{ fail \"must never execute\" }}\n---\nkind: Secret\nmetadata:\n  name: injected";
    req["profileConfig"]["nodePools"][0]["subnets"]["tag"] = json!({"key":key,"value":value});
    let docs = rendered_configuration(&req);
    assert_eq!(docs.len(), 4);
    assert_eq!(
        resource(&docs, "EC2NodeClass", "qovery-general")["spec"]["subnetSelectorTerms"][0]["tags"],
        json!({key:value})
    );
}

#[test]
fn empty_taint_value_is_omitted_to_satisfy_the_pinned_crd_without_changing_its_meaning() {
    let mut req = request("configuration");
    req["profileConfig"]["nodePools"][1]["taints"][0]["value"] = json!("");
    let docs = rendered_configuration(&req);
    assert_eq!(
        resource(&docs, "NodePool", "qovery-isolated")["spec"]["template"]["spec"]["taints"],
        json!([{"key":"dedicated", "effect":"NoSchedule"}])
    );
}

#[test]
fn configuration_chart_has_no_implicit_pools_and_crd_component_uses_the_same_pinned_version() {
    assert!(
        parse_yaml_documents(&helm_template("karpenter-configuration", CONFIG_CHART, "kube-system", &[], &[]))
            .is_empty()
    );
    let docs = parse_yaml_documents(&helm_template(
        "karpenter-crd",
        "lib-engine/lib/aws/bootstrap/charts/karpenter-crd",
        "kube-system",
        &[
            repository_path("platform-catalog/components/karpenter-crd/config/static-values/base.yaml"),
            repository_path("platform-catalog/components/karpenter-crd/config/runtime-values/managed-values.yaml"),
        ],
        &["--include-crds"],
    ));
    for name in [
        "nodepools.karpenter.sh",
        "nodeclaims.karpenter.sh",
        "ec2nodeclasses.karpenter.k8s.aws",
    ] {
        assert!(document_by_kind_and_name(&docs, "CustomResourceDefinition", name).is_some());
    }
    for chart in [
        CONFIG_CHART,
        CONTROLLER_CHART,
        "lib-engine/lib/aws/bootstrap/charts/karpenter-crd",
    ] {
        let metadata: Value =
            serde_yaml::from_str(&fs::read_to_string(repository_path(format!("{chart}/Chart.yaml"))).unwrap()).unwrap();
        assert_eq!(metadata["appVersion"], "1.10.0");
    }
}

const QOVERY_CONFIG_CHART: &str = "lib-engine/lib/aws/bootstrap/charts/karpenter-configuration";
const QOVERY_CONFIG_BASE: &str =
    "platform-catalog/components/karpenter-qovery-configuration/config/static-values/base.yaml";
const LEGACY_EFFECTIVE_VALUES: &str =
    "platform-catalog/components/karpenter-qovery-configuration/tests/legacy-effective.values.yaml";

fn rendered_qovery_configuration(req: &Value, preserved_values: &[PathBuf]) -> Vec<Value> {
    let directory = tempdir().unwrap();
    let values = directory.path().join("compiled.yaml");
    fs::write(&values, serde_yaml::to_string(&compiled(req)).unwrap()).unwrap();
    let mut layers = vec![repository_path(QOVERY_CONFIG_BASE)];
    layers.extend_from_slice(preserved_values);
    layers.push(values);
    parse_yaml_documents(&helm_template(
        "karpenter-configuration",
        QOVERY_CONFIG_CHART,
        "kube-system",
        &layers,
        &[],
    ))
    .into_iter()
    .map(|value| serde_json::to_value(value).unwrap())
    .collect()
}

#[test]
fn qovery_default_stable_share_a_class_and_preserve_legacy_policy_except_stable_consolidation() {
    let req = request("qovery-configuration");
    let baseline = parse_yaml_documents(&helm_template(
        "karpenter-configuration",
        QOVERY_CONFIG_CHART,
        "kube-system",
        &[repository_path(LEGACY_EFFECTIVE_VALUES)],
        &[],
    ));
    let mut expected: Vec<Value> = baseline
        .into_iter()
        .map(|value| serde_json::to_value(value).unwrap())
        .collect();
    let stable = expected
        .iter_mut()
        .find(|value| value["kind"] == "NodePool" && value["metadata"]["name"] == "stable")
        .unwrap();
    // No model means unchanged chart/legacy policy, including the old budget supplied by q-core.
    assert_eq!(stable["spec"]["disruption"]["consolidationPolicy"], "WhenEmptyOrUnderutilized");
    assert_eq!(stable["spec"]["disruption"]["budgets"].as_array().unwrap().len(), 2);
    stable["spec"]["disruption"]["consolidationPolicy"] = json!("WhenEmpty");
    stable["spec"]["disruption"]["budgets"] = json!([{"nodes":"10%"}]);
    let actual = rendered_qovery_configuration(&req, &[repository_path(LEGACY_EFFECTIVE_VALUES)]);
    assert_eq!(
        actual, expected,
        "a reviewed static overlay keeps non-exposed settings; only the stable policy/calendar changes"
    );
    assert_eq!(actual.len(), 3);
    for name in ["default", "stable"] {
        assert_eq!(
            resource(&actual, "NodePool", name)["spec"]["template"]["spec"]["nodeClassRef"]["name"],
            "default"
        );
    }
    let class = resource(&actual, "EC2NodeClass", "default");
    assert_eq!(class["spec"]["role"], "KarpenterNodeRole-example-eks");
    assert_eq!(class["spec"]["blockDeviceMappings"][0]["ebs"]["iops"], 3500);
    assert_eq!(class["spec"]["blockDeviceMappings"][0]["ebs"]["throughput"], 200);
    assert_eq!(class["spec"]["tags"]["Owner"], "platform-team");
    assert_eq!(class["spec"]["tags"]["aws-apn-id"], "pc:synthetic-example");
}

#[test]
fn qovery_spot_is_independent_and_stable_defaults_to_empty_only_after_thirty_seconds() {
    for default_spot in [false, true] {
        for stable_spot in [false, true] {
            let mut req = request("qovery-configuration");
            req["profileConfig"]["default"]["spotEnabled"] = json!(default_spot);
            req["profileConfig"]["stable"]["spotEnabled"] = json!(stable_spot);
            req["profileConfig"]["diskSizeGiB"] = json!(80);
            req["clusterInputs"]["aws.nodeRoleName"] = json!("existing-custom-node-role");
            let docs = rendered_qovery_configuration(&req, &[]);
            for (name, spot, weight) in [("default", default_spot, 50), ("stable", stable_spot, 10)] {
                let pool = resource(&docs, "NodePool", name);
                let requirements = pool["spec"]["template"]["spec"]["requirements"].as_array().unwrap();
                let capacity = requirements
                    .iter()
                    .find(|value| value["key"] == "karpenter.sh/capacity-type")
                    .unwrap();
                assert_eq!(
                    capacity["values"],
                    if spot {
                        json!(["spot", "on-demand"])
                    } else {
                        json!(["on-demand"])
                    }
                );
                assert_eq!(pool["spec"]["weight"], weight);
                assert_eq!(pool["spec"]["template"]["spec"]["expireAfter"], "720h");
            }
            let stable = resource(&docs, "NodePool", "stable");
            assert_eq!(
                stable["spec"]["disruption"],
                json!({"consolidationPolicy":"WhenEmpty", "consolidateAfter":"30s", "budgets":[{"nodes":"10%"}]})
            );
            assert_eq!(
                stable["spec"]["template"]["spec"]["taints"],
                json!([{"key":"nodepool/stable", "effect":"NoSchedule"}])
            );
            let class = resource(&docs, "EC2NodeClass", "default");
            assert_eq!(class["spec"]["role"], "existing-custom-node-role");
            assert_eq!(class["spec"]["blockDeviceMappings"][0]["ebs"]["volumeSize"], "80Gi");
            assert_eq!(
                class["spec"]["subnetSelectorTerms"],
                json!([{"tags":{"karpenter.sh/discovery":"example-eks"}}])
            );
        }
    }
}

#[test]
fn qovery_model_rejects_partial_lists_disk_and_spot_without_compiling() {
    let cases = [
        (
            "/instanceRequirements/architectures",
            json!([]),
            "instanceRequirements.architectures",
        ),
        (
            "/instanceRequirements/architectures",
            json!(["riscv"]),
            "instanceRequirements.architectures[0]",
        ),
        (
            "/instanceRequirements/families",
            json!(["m6i", "m6i"]),
            "instanceRequirements.families",
        ),
        ("/instanceRequirements/sizes", json!([]), "instanceRequirements.sizes"),
        ("/diskSizeGiB", json!(19), "diskSizeGiB"),
        ("/diskSizeGiB", json!(20.5), "diskSizeGiB"),
        ("/stable/spotEnabled", json!("false"), "stable.spotEnabled"),
    ];
    for (pointer, value, path) in cases {
        for operation in ["VALIDATE", "COMPILE"] {
            let mut req = request("qovery-configuration");
            req["operation"] = json!(operation);
            *req["profileConfig"].pointer_mut(pointer).unwrap() = value.clone();
            let result = evaluate(&req);
            assert!(
                result["violations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|value| value["fieldPath"] == path),
                "{result}"
            );
            assert!(result.get("helmValues").is_none());
        }
    }
    let mut req = request("qovery-configuration");
    req["profileConfig"] = json!({});
    assert!(evaluate(&req).get("helmValues").is_none());
}

#[test]
fn qovery_logical_inputs_are_explicit_and_core_metadata_is_compile_only() {
    for operation in ["DESCRIBE", "RESOLVE_REQUIREMENTS", "VALIDATE", "COMPILE"] {
        let mut req = request("qovery-configuration");
        req["operation"] = json!(operation);
        if operation == "DESCRIBE" {
            req["clusterContext"] = Value::Null;
        }
        if operation != "COMPILE" {
            req["clusterInputs"]
                .as_object_mut()
                .unwrap()
                .retain(|key, _| key.starts_with("aws."));
        }
        let result = evaluate(&req);
        assert_eq!(result["violations"], json!([]));
        assert_eq!(
            result["requiredInputs"].as_array().unwrap().len(),
            if operation == "DESCRIBE" { 0 } else { 4 }
        );
    }
    let mut req = request("qovery-configuration");
    req["clusterInputs"] = json!({});
    let result = evaluate(&req);
    assert!(result.get("helmValues").is_none());
    assert!(
        result["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["fieldPath"] == "clusterInputs.aws.nodeRoleName")
    );
    assert!(
        result["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["fieldPath"] == "clusterInputs.cluster.id")
    );
}

#[test]
fn qovery_bottlerocket_alias_keeps_the_legacy_os_and_data_disk_layout() {
    let mut req = request("qovery-configuration");
    req["clusterInputs"]["aws.amiSelectorTermsAlias"] = json!("bottlerocket@v1.44.0");
    let docs = rendered_qovery_configuration(&req, &[]);
    let class = resource(&docs, "EC2NodeClass", "default");
    assert_eq!(class["spec"]["amiSelectorTerms"], json!([{"alias":"bottlerocket@v1.44.0"}]));
    let volumes = class["spec"]["blockDeviceMappings"].as_array().unwrap();
    assert_eq!(volumes.len(), 2);
    assert_eq!(volumes[0]["deviceName"], "/dev/xvda");
    assert_eq!(volumes[0]["ebs"]["volumeSize"], "4Gi");
    assert_eq!(volumes[1]["deviceName"], "/dev/xvdb");
    assert_eq!(volumes[1]["ebs"]["volumeSize"], "50Gi");
}

#[test]
fn custom_pools_absent_or_empty_need_no_aws_inputs_and_render_no_resources() {
    for config in [json!({}), json!({"nodePools":[]})] {
        for inputs in [
            json!({}),
            json!({"aws.eksClusterName":false, "aws.nodeRoleName":"invalid role", "aws.nodeSecurityGroupId":"invalid"}),
        ] {
            let mut req = request("configuration");
            req["profileConfig"] = config.clone();
            req["clusterInputs"] = inputs.clone();
            req["clusterContext"] = Value::Null;
            for operation in ["DESCRIBE", "RESOLVE_REQUIREMENTS", "VALIDATE", "COMPILE"] {
                req["operation"] = json!(operation);
                let result = evaluate(&req);
                assert_eq!(result["requiredInputs"], json!([]), "{result}");
                assert_eq!(result["violations"], json!([]), "{result}");
                assert_eq!(result["fields"][0]["required"], false);
                if operation == "COMPILE" {
                    assert_eq!(result["helmValues"], json!({"pools":[]}));
                    assert!(rendered_configuration(&req).is_empty());
                } else {
                    assert!(result.get("helmValues").is_none());
                }
            }
        }
    }
}

#[test]
fn one_custom_pool_activates_aws_requirements_and_cannot_compile_an_incomplete_row() {
    let mut req = request("configuration");
    req["profileConfig"] = json!({"nodePools":[{}]});
    req["clusterInputs"] = json!({});
    for operation in ["RESOLVE_REQUIREMENTS", "VALIDATE", "COMPILE"] {
        req["operation"] = json!(operation);
        let result = evaluate(&req);
        assert_eq!(result["requiredInputs"].as_array().unwrap().len(), 3);
        assert!(result.get("helmValues").is_none());
        if operation != "RESOLVE_REQUIREMENTS" {
            for path in [
                "nodePools[0].name",
                "nodePools[0].instanceTypes",
                "clusterInputs.aws.nodeRoleName",
            ] {
                assert!(
                    result["violations"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|entry| entry["fieldPath"] == path),
                    "{result}"
                );
            }
        }
    }
    req = request("configuration");
    req["profileConfig"] = json!({"nodePools":[{"name":"extra", "instanceTypes":["m7i.large"]}]});
    assert_eq!(rendered_configuration(&req).len(), 2);
}

#[test]
fn regional_reference_data_supplies_choices_and_rejects_unknown_instance_types() {
    let mut req = request("configuration");
    req["operation"] = json!("RESOLVE_REQUIREMENTS");
    let result = evaluate(&req);
    let instance_types = result["fields"][0]["itemFields"][0]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["key"] == "instanceTypes")
        .unwrap();
    assert_eq!(
        instance_types["items"]["constraints"]["allowedValues"],
        json!(["c7i-flex.large", "c7i.metal-24xl", "m7i.large"])
    );

    req["referenceData"]["computeInstances"] = json!([
        {"name": "m5.large", "architecture": "amd64", "family": "m5", "size": "large"}
    ]);
    for operation in ["RESOLVE_REQUIREMENTS", "VALIDATE", "COMPILE"] {
        req["operation"] = json!(operation);
        let result = evaluate(&req);
        assert!(result["violations"].as_array().unwrap().iter().any(|violation| {
            violation["code"] == "VALUE_NOT_ALLOWED" && violation["fieldPath"] == "nodePools[0].instanceTypes[0]"
        }));
        assert!(result.get("helmValues").is_none());
    }
}

fn raw_pool(name: &str) -> Value {
    json!({
        "apiVersion": "karpenter.sh/v1", "kind": "NodePool", "metadata": {"name": name},
        "spec": {
            "template": {"spec": {
                "nodeClassRef": {"group": "karpenter.k8s.aws", "kind": "EC2NodeClass", "name": "shared-class"},
                "requirements": [{"key": "kubernetes.io/arch", "operator": "In", "values": ["arm64"]}]
            }},
            "disruption": {"consolidationPolicy": "WhenEmpty", "consolidateAfter": "5m"}
        }
    })
}

fn with_resources(mut req: Value, resources: &[Value]) -> Value {
    req["profileConfig"]["resources"] = json!(
        resources
            .iter()
            .map(|value| { json!({"manifest": serde_yaml::to_string(value).unwrap()}) })
            .collect::<Vec<_>>()
    );
    req
}

#[test]
fn raw_resources_preserve_specs_and_literal_helm_text_alongside_guided_pools() {
    let pool = raw_pool("yaml-pool");
    let class = json!({
        "apiVersion": "karpenter.k8s.aws/v1", "kind": "EC2NodeClass",
        "metadata": {"name": "shared-class", "annotations": {"example.com/note": "{{ .Release.Name }}"}},
        "spec": {
            "role": "existing-node-role", "amiSelectorTerms": [{"alias": "al2023@v20260814"}],
            "subnetSelectorTerms": [{"tags": {"custom": "discovery"}}],
            "securityGroupSelectorTerms": [{"id": "sg-0123456789abcdef0"}],
            "userData": "#!/bin/bash\necho '{{ literal user data }}'\n",
            "kubelet": {"maxPods": 40}
        }
    });
    let req = with_resources(request("configuration"), &[pool.clone(), class.clone()]);
    let docs = rendered_configuration(&req);
    assert_eq!(docs.len(), 6);
    assert_eq!(resource(&docs, "NodePool", "yaml-pool"), &pool);
    assert_eq!(resource(&docs, "EC2NodeClass", "shared-class"), &class);
    for original in rendered_configuration(&request("configuration")) {
        assert!(docs.contains(&original), "guided resource changed: {original}");
    }
}

#[test]
fn raw_only_configuration_needs_no_guided_aws_inputs_or_reference_data() {
    let mut req = request("configuration");
    req["profileConfig"] = json!({});
    req["clusterInputs"] = json!({});
    req["referenceData"] = Value::Null;
    // Sharing a class does not require it to be created in this draft.
    let req = with_resources(req, &[raw_pool("first"), raw_pool("second")]);
    let result = evaluate(&req);
    assert_eq!(result["requiredInputs"], json!([]));
    assert_eq!(rendered_configuration(&req).len(), 2);
}

#[test]
fn yaml_editor_metadata_exposes_incomplete_starters_without_defaulting_resources() {
    let mut req = request("configuration");
    req["profileConfig"] = json!({});
    req["operation"] = json!("DESCRIBE");
    let result = evaluate(&req);
    let manifest = &result["fields"][1]["items"]["fields"][0];
    assert_eq!(manifest["format"], "kubernetes-resource-yaml");
    assert!(manifest.get("defaultValue").is_none());
    let templates = manifest["templates"].as_array().unwrap();
    assert_eq!(templates.len(), 2);
    for template in templates {
        let skeleton: Value = serde_yaml::from_str(template["value"].as_str().unwrap()).unwrap();
        assert_eq!(skeleton["metadata"], json!({"name": ""}));
        assert!(skeleton.get("status").is_none());
        assert!(skeleton["spec"].is_object());
        let crd_file = match template["id"].as_str().unwrap() {
            "nodepool" => "karpenter.sh_nodepools.yaml",
            "ec2nodeclass" => "karpenter.k8s.aws_ec2nodeclasses.yaml",
            other => panic!("unexpected template: {other}"),
        };
        let crd: Value = serde_yaml::from_slice(
            &fs::read(repository_path(format!(
                "lib-engine/lib/aws/bootstrap/charts/karpenter/crds/{crd_file}"
            )))
            .unwrap(),
        )
        .unwrap();
        let version = crd["spec"]["versions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "v1")
            .unwrap();
        assert_eq!(skeleton["apiVersion"], format!("{}/v1", crd["spec"]["group"].as_str().unwrap()));
        assert_eq!(skeleton["kind"], crd["spec"]["names"]["kind"]);
        assert_skeleton_structure(
            &skeleton["spec"],
            &version["schema"]["openAPIV3Schema"]["properties"]["spec"],
            "spec",
        );

        let mut incomplete = with_resources(request("configuration"), &[skeleton]);
        incomplete["operation"] = json!("COMPILE");
        let rejected = evaluate(&incomplete);
        assert!(rejected.get("helmValues").is_none(), "starter must require editing");
    }
    req["operation"] = json!("COMPILE");
    assert_eq!(compiled(&req), json!({"pools": []}));
    assert!(rendered_configuration(&req).is_empty());
}

#[test]
fn resource_collisions_fail_before_rendering_with_indexed_errors() {
    for (documents, path) in [
        (vec![raw_pool("default")], "resources[0].manifest"),
        (vec![raw_pool("stable")], "resources[0].manifest"),
        (vec![raw_pool("qovery-general")], "resources[0].manifest"),
        (vec![raw_pool("same"), raw_pool("same")], "resources[1].manifest"),
        (
            vec![
                json!({"apiVersion":"karpenter.k8s.aws/v1", "kind":"EC2NodeClass", "metadata":{"name":"default"}, "spec":{}}),
            ],
            "resources[0].manifest",
        ),
    ] {
        let result = evaluate(&with_resources(request("configuration"), &documents));
        assert!(result.get("helmValues").is_none(), "{result}");
        assert!(
            result["violations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| { v["code"] == "RESOURCE_NAME_CONFLICT" && v["fieldPath"] == path }),
            "{result}"
        );
    }
    // Identity includes kind: a pool and a class may legitimately share a name.
    let same_name =
        json!({"apiVersion":"karpenter.k8s.aws/v1", "kind":"EC2NodeClass", "metadata":{"name":"pair"}, "spec":{}});
    assert_eq!(
        compiled(&with_resources(request("configuration"), &[raw_pool("pair"), same_name]))["resources"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn raw_document_shape_and_bounds_fail_closed_without_crd_validation() {
    let mut namespaced = raw_pool("test");
    namespaced["metadata"]["namespace"] = json!("kube-system");
    let mut runtime = raw_pool("test");
    runtime["status"] = json!({});
    let mut missing_spec = raw_pool("test");
    missing_spec.as_object_mut().unwrap().remove("spec");
    for document in [
        json!([]),
        json!(null),
        json!({"apiVersion":"v1", "kind":"Secret", "metadata":{"name":"test"}, "spec":{}}),
        namespaced,
        runtime,
        missing_spec,
    ] {
        let result = evaluate(&with_resources(request("configuration"), &[document]));
        assert!(result.get("helmValues").is_none());
        assert!(
            result["violations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["code"] == "INVALID_RESOURCE" && v["fieldPath"] == "resources[0].manifest"),
            "{result}"
        );
    }
    for manifest in ["---\n---\n".to_owned(), " ".repeat(65537)] {
        let mut req = request("configuration");
        req["profileConfig"]["resources"] = json!([{"manifest":manifest}]);
        let result = evaluate(&req);
        assert!(result.get("helmValues").is_none(), "{result}");
    }
    let resources: Vec<_> = (0..33).map(|i| raw_pool(&format!("pool-{i}"))).collect();
    let result = evaluate(&with_resources(request("configuration"), &resources));
    assert!(result.get("helmValues").is_none());
    assert!(
        result["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["code"] == "LENGTH_OUT_OF_RANGE" && v["fieldPath"] == "resources")
    );
}

// Starter values are intentionally incomplete; only check required field structure and known
// properties against the pinned schemas, not scalar validity, selectors or CEL admission rules.
fn assert_skeleton_structure(value: &Value, schema: &Value, path: &str) {
    if let Some(fields) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for key in required {
                assert!(fields.contains_key(key.as_str().unwrap()), "missing {path}.{key}");
            }
        }
        for (key, child) in fields {
            let child_schema = schema["properties"]
                .get(key)
                .unwrap_or_else(|| panic!("unknown starter field {path}.{key}"));
            assert_skeleton_structure(child, child_schema, &format!("{path}.{key}"));
        }
    } else if let Some(items) = value.as_array() {
        for child in items {
            assert_skeleton_structure(child, &schema["items"], &format!("{path}[]"));
        }
    }
}

#[test]
fn malformed_yaml_resolves_guided_pool_inputs_without_parsing_resources() {
    let mut req = request("configuration");
    req["operation"] = json!("RESOLVE_REQUIREMENTS");
    let expected = evaluate(&req);
    assert_eq!(expected["requiredInputs"].as_array().unwrap().len(), 3);
    req["profileConfig"]["resources"] = json!([{"manifest": "spec: [unterminated"}]);
    let resolved = evaluate(&req);
    assert_eq!(resolved["requiredInputs"], expected["requiredInputs"]);
    assert_eq!(resolved["fields"][1]["itemFields"][0][0]["format"], "kubernetes-resource-yaml");
    assert!(resolved.get("helmValues").is_none());
}

#[test]
fn malformed_yaml_still_describes_the_editor_but_cannot_compile() {
    let mut req = request("configuration");
    req["profileConfig"]["resources"] = json!([{"manifest": "spec: [unterminated"}]);
    req["operation"] = json!("DESCRIBE");
    let described = evaluate(&req);
    assert_eq!(described["fields"][1]["itemFields"][0][0]["format"], "kubernetes-resource-yaml");
    // Until q-core adds indexed syntax prevalidation this intentionally fails evaluation closed.
    req["operation"] = json!("COMPILE");
    let isolated = tempdir().unwrap();
    copy_bundle(
        &repository_path("platform-catalog/components/karpenter-configuration/config/runtime-values"),
        isolated.path(),
    );
    let output = Command::new(std::env::var_os("PKL_BIN").unwrap_or_else(|| "pkl".into()))
        .args(["eval", "--allowed-resources=^prop:request$", "-p"])
        .arg(format!("request={req}"))
        .arg(isolated.path().join("model.pkl"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "no partial Helm values may escape");
}
