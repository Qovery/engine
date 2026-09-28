use qovery_engine::tera_utils::render_one_off;
use tera::Context;

const EKS_NODE_CUSTOMER_IAM_POLICIES_TEMPLATE: &str =
    include_str!("../lib/aws/bootstrap/terraform/eks-node-customer-iam-policies.j2.tf");

fn render_node_policies(policy_arns: &[&str]) -> String {
    let mut context = Context::new();
    context.insert("aws_eks_node_iam_policy_arns", &policy_arns);
    render_one_off(EKS_NODE_CUSTOMER_IAM_POLICIES_TEMPLATE, &context).expect("node IAM policy template should render")
}

fn rendered_policy_arns(rendered: &str) -> Vec<String> {
    let encoded_arns = rendered
        .lines()
        .find_map(|line| line.trim().strip_prefix("customer_node_iam_policy_arns = toset("))
        .and_then(|line| line.strip_suffix(')'))
        .expect("rendered template should define the policy ARN set");
    serde_json::from_str(encoded_arns).expect("policy ARNs should be encoded as a JSON list")
}

#[test]
fn empty_policy_list_renders_no_attachment_instances() {
    let rendered = render_node_policies(&[]);

    assert!(rendered_policy_arns(&rendered).is_empty());
    assert_eq!(
        rendered
            .matches("for_each   = local.customer_node_iam_policy_arns")
            .count(),
        2
    );
}

#[test]
fn customer_policies_render_for_both_node_roles() {
    let policy_arns = [
        "arn:aws:iam::123456789012:policy/team/EcrCache",
        "arn:aws:iam::123456789012:policy/team/Observability",
    ];
    let rendered = render_node_policies(&policy_arns);

    assert_eq!(rendered_policy_arns(&rendered), policy_arns);
    assert!(rendered.contains("role       = aws_iam_role.eks_workers.name"));
    assert!(rendered.contains("role       = aws_iam_role.karpenter_node_role.name"));
    assert_eq!(
        rendered
            .matches("for_each   = local.customer_node_iam_policy_arns")
            .count(),
        2
    );
    assert_eq!(
        rendered
            .matches(
                "data.aws_partition.current.partition}:iam::${data.aws_caller_identity.current.account_id}:policy/"
            )
            .count(),
        2
    );
}
