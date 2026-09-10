//! Unpublished Karpenter models evaluated through the same isolated entrypoint as q-core,
//! then rendered with the actual vendored Helm charts. No AWS or Kubernetes access.

use platform_catalog_tests::{
    assert_success, document_by_kind_and_name, helm_template, parse_yaml_documents, repository_path, yaml_path,
};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
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
    let catalog: Value =
        serde_yaml::from_str(&fs::read_to_string(repository_path("platform-catalog/catalog.yaml")).unwrap()).unwrap();
    assert!(
        !catalog["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|component| component["name"].as_str().unwrap().starts_with("karpenter")),
        "activation requires the execution and ownership gates from subsequent lots"
    );
}
