//! Enforces module layering for published Pkl bundles and unpublished exchange fixtures.
//!
//! The decomposition documented in each component's `config/README.md` is only real if it survives
//! the next contributor:
//!
//! ```text
//! contract.pkl, sdk/     vocabulary and the shared evaluator (vendored, machine-synced)
//! <setting>/             one folder per setting: setting.pkl (what it is),
//!                        dependencies.pkl (what it needs), helm.pkl (the Helm it owns)
//! settings.pkl           table of contents: one Feature per folder
//! model.pkl              decodes prop:request, hands the component to the SDK, renders JSON
//! ```
//!
//! The rule that matters most is FOLDER -> ROOT: a setting folder may only reach up to the modules
//! in [`ROOT_IMPORTS_ALLOWED_FROM_FEATURES`]. Without it, a default parked in a root module silently
//! turns every folder into a dependant of it — which is the regression this check exists to prevent.
//! Folders also stay independent of each other: a folder reads other settings through the resolved
//! configuration (`scope.config`), never through their modules.
//!
//! `sdk/` is not a feature package: it is the vendored copy of the component-agnostic authoring
//! SDK (`platform-catalog/pkl/sdk`), synced by `sync-platform-pkl-sdk.sh`. Every module may import
//! it, and it may import nothing of the component except the vendored contract — a component
//! module reached from `sdk/` would make the shared copy impossible to vendor byte-identically.
//!
//! The rules are applied by [`check`], a pure function over parsed modules, so the tests at the
//! bottom of this file prove each rule actually fires instead of asserting the checker merely runs.

use platform_catalog_tests::repository_path;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Root modules a setting folder may import. Shrinking this list is the ratchet: each entry removed
/// is one less way for a folder to depend on the root. Only the vendored contract is left — a folder
/// receives every draft value through the evaluation scope.
const ROOT_IMPORTS_ALLOWED_FROM_FEATURES: &[&str] = &["contract.pkl"];

/// The vendored SDK package. Any module may import it; it may import only the vendored contract,
/// so the same bytes stay valid in every component bundle.
const SDK_PACKAGE: &str = "sdk";
const SDK_ROOT_IMPORTS_ALLOWED: &[&str] = &["contract.pkl"];

/// The entrypoint stays an I/O shim so the native Pkl tests exercise the same code path q-core does:
/// it decodes the request with the SDK and hands its `settings.pkl` component to the SDK evaluator.
const MODEL_IMPORTS_ALLOWED: &[&str] = &["sdk/request.pkl", "sdk/evaluate.pkl", "settings.pkl"];

const BUNDLE_DIR: &str = "config/runtime-values";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Import {
    /// Path exactly as written in the source, e.g. `../contract.pkl`.
    written: String,
    /// Path relative to the bundle root, e.g. `contract.pkl`.
    resolved: String,
    alias: Option<String>,
    line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Module {
    component: String,
    /// Path relative to the bundle root, e.g. `storage/helm.pkl`.
    relative: String,
    imports: Vec<Import>,
}

impl Module {
    /// The feature package a module belongs to, or `None` when it sits at the bundle root.
    fn package(&self) -> Option<&str> {
        self.relative.rsplit_once('/').map(|(dir, _)| dir)
    }
}

fn package_of(resolved: &str) -> Option<&str> {
    resolved.rsplit_once('/').map(|(dir, _)| dir)
}

/// Resolve an import written relative to the importing module into a bundle-root-relative path.
fn resolve(importer_package: Option<&str>, written: &str) -> String {
    match written.strip_prefix("../") {
        Some(rest) => rest.to_string(),
        None => match importer_package {
            Some(package) => format!("{package}/{written}"),
            None => written.to_string(),
        },
    }
}

/// `storage` + `types.pkl` -> `storageTypes`.
fn expected_alias(package: &str, resolved: &str) -> String {
    let stem = Path::new(resolved)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let mut characters = stem.chars();
    match characters.next() {
        Some(first) => format!("{package}{}{}", first.to_uppercase(), characters.as_str()),
        None => package.to_string(),
    }
}

fn parse_module(component: &str, relative: &str, source: &str) -> Module {
    let imports = source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| parse_import(line).map(|(written, alias)| (index + 1, written, alias)))
        .map(|(line, written, alias)| Import {
            resolved: resolve(relative.rsplit_once('/').map(|(dir, _)| dir), &written),
            written,
            alias,
            line,
        })
        .collect();

    Module {
        component: component.to_string(),
        relative: relative.to_string(),
        imports,
    }
}

/// Parse `import "<path>"` and `import "<path>" as <alias>`. Anything else is not an import.
/// Leading whitespace is trimmed: Pkl accepts an indented top-level import, and skipping it here
/// would let that import bypass every layering rule.
fn parse_import(line: &str) -> Option<(String, Option<String>)> {
    let rest = line.trim_start().strip_prefix("import ")?;
    let rest = rest.strip_prefix('"')?;
    let (written, rest) = rest.split_once('"')?;
    let alias = rest
        .trim()
        .strip_prefix("as ")
        .map(|alias| alias.trim().to_string())
        .filter(|alias| !alias.is_empty());
    Some((written.to_string(), alias))
}

fn check(modules: &[Module]) -> Vec<String> {
    let mut violations = Vec::new();

    for module in modules {
        let location = format!("{}/{}", module.component, module.relative);

        for import in &module.imports {
            let at = format!("{location}:{}", import.line);

            if module.relative == "model.pkl" && !MODEL_IMPORTS_ALLOWED.contains(&import.written.as_str()) {
                violations.push(format!(
                    "{at}: entrypoint imports '{}'; model.pkl may only import {}",
                    import.written,
                    MODEL_IMPORTS_ALLOWED.join(", ")
                ));
            }

            // Deliberately above the pkl: skip — the stdlib is not exempt for the contract: any
            // import there means logic creeping into the shared vocabulary.
            if module.relative == "contract.pkl" {
                violations.push(format!(
                    "{at}: the vendored contract imports '{}'; it is synced by \
                     sync-platform-pkl-sdk.sh and must import nothing",
                    import.written
                ));
            }

            if import.written.starts_with("pkl:") {
                continue;
            }

            let target_package = package_of(&import.resolved);

            if let Some(importer_package) = module.package() {
                let importer_is_sdk = importer_package == SDK_PACKAGE;
                let allowed_root_imports = if importer_is_sdk {
                    SDK_ROOT_IMPORTS_ALLOWED
                } else {
                    ROOT_IMPORTS_ALLOWED_FROM_FEATURES
                };
                match target_package {
                    None if !allowed_root_imports.contains(&import.resolved.as_str()) => {
                        let importer_kind = if importer_is_sdk {
                            "the vendored SDK"
                        } else {
                            "feature package"
                        };
                        violations.push(format!(
                            "{at}: {importer_kind} imports root module '{}'; it may only import {}",
                            import.resolved,
                            allowed_root_imports.join(", ")
                        ));
                    }
                    // Every module may use the vendored SDK; the SDK itself must stay
                    // component-agnostic, and feature packages must stay independent.
                    Some(target) if target != importer_package && target != SDK_PACKAGE => {
                        if importer_is_sdk {
                            violations.push(format!(
                                "{at}: the vendored SDK imports '{}'; sdk modules may only import \
                                 contract.pkl and other sdk modules",
                                import.resolved
                            ));
                        } else if import.resolved.ends_with("/setting.pkl") {
                            // A folder reads a sibling setting through its declaration, and only
                            // through it; cycles are rejected below.
                        } else {
                            violations.push(format!(
                                "{at}: imports '{}' from feature package '{target}'; a folder may \
                                 only import a sibling's setting.pkl",
                                import.resolved
                            ));
                        }
                    }
                    _ => {}
                }
            }

            match target_package {
                Some(target) if Some(target) != module.package() => {
                    let expected = expected_alias(target, &import.resolved);
                    if import.alias.as_deref() != Some(expected.as_str()) {
                        violations.push(format!(
                            "{at}: imports '{}' as '{}'; cross-package imports must be aliased '{expected}'",
                            import.resolved,
                            import.alias.as_deref().unwrap_or("<no alias>")
                        ));
                    }
                }
                _ => {
                    if let Some(alias) = &import.alias {
                        violations.push(format!(
                            "{at}: imports '{}' as '{alias}'; same-package imports must not be aliased",
                            import.resolved
                        ));
                    }
                }
            }
        }
    }

    violations.extend(setting_import_cycles(modules));
    violations
}

/// Typed cross-setting reads go through a sibling's `setting.pkl`. Per bundle they form a graph
/// that must stay acyclic: two folders defining each other would have no readable order.
fn setting_import_cycles(modules: &[Module]) -> Vec<String> {
    let mut edges: BTreeMap<(&str, &str), BTreeSet<&str>> = BTreeMap::new();
    for module in modules {
        let Some(package) = module.package() else { continue };
        if package == SDK_PACKAGE {
            continue;
        }
        for import in &module.imports {
            if let Some(target) = package_of(&import.resolved)
                && target != package
                && target != SDK_PACKAGE
                && import.resolved.ends_with("/setting.pkl")
            {
                edges
                    .entry((module.component.as_str(), package))
                    .or_default()
                    .insert(target);
            }
        }
    }

    let mut violations = Vec::new();
    for (component, start) in edges.keys() {
        let mut stack: Vec<(&str, Vec<&str>)> = vec![(start, vec![start])];
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        while let Some((node, path)) = stack.pop() {
            for target in edges.get(&(component, node)).into_iter().flatten() {
                if target == start {
                    // Report each cycle once, from its alphabetically first folder.
                    if path.iter().all(|folder| start <= folder) {
                        violations.push(format!(
                            "{component}: setting folders import each other in a cycle: {} -> {target}",
                            path.join(" -> ")
                        ));
                    }
                } else if seen.insert(target) {
                    let mut next = path.clone();
                    next.push(target);
                    stack.push((target, next));
                }
            }
        }
    }
    violations
}

fn load_modules() -> Vec<Module> {
    let mut modules = load_modules_from(&repository_path("platform-catalog/components"));
    modules.extend(load_modules_from(&repository_path("platform-catalog/pkl/tests/fixtures")));
    modules
}

fn load_modules_from(components: &Path) -> Vec<Module> {
    let mut modules = Vec::new();

    let mut entries: Vec<PathBuf> = fs::read_dir(components)
        .unwrap_or_else(|error| panic!("cannot read bundle inventory {components:?}: {error}"))
        .map(|entry| entry.expect("component directory entry must be readable").path())
        .collect();
    entries.sort();

    for component_dir in entries {
        let bundle = component_dir.join(BUNDLE_DIR);
        if !bundle.is_dir() {
            continue;
        }
        let component = component_dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("component directory must have a name")
            .to_string();

        for path in pkl_files(&bundle) {
            let relative = path
                .strip_prefix(&bundle)
                .expect("module must live below the bundle root")
                .to_str()
                .expect("module path must be valid UTF-8")
                .replace('\\', "/");
            let source = fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {path:?}: {error}"));
            modules.push(parse_module(&component, &relative, &source));
        }
    }

    modules
}

fn pkl_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut entries: Vec<PathBuf> = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {directory:?}: {error}"))
        .map(|entry| entry.expect("directory entry must be readable").path())
        .collect();
    entries.sort();

    for path in entries {
        if path.is_dir() {
            files.extend(pkl_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "pkl") {
            files.push(path);
        }
    }
    files
}

#[test]
fn published_bundles_respect_the_module_layering() {
    let modules = load_modules();
    assert!(
        !modules.is_empty(),
        "no platform configuration module was found; the loader is looking in the wrong place"
    );

    let violations = check(&modules);
    assert!(violations.is_empty(), "{}", report(&violations));
}

#[test]
fn every_evaluator_bundle_declares_an_entrypoint() {
    let modules = load_modules();
    let components: Vec<&str> = modules
        .iter()
        .filter(|module| module.relative == "model.pkl")
        .map(|module| module.component.as_str())
        .collect();

    assert!(
        components.contains(&"loki") && components.contains(&"cluster-agent"),
        "expected loki and cluster-agent to expose runtime-values/model.pkl, found {components:?}"
    );
    assert!(
        components.contains(&"karpenter-v1"),
        "the unpublished structured-field fixture must be covered by the same layering rules"
    );
}

fn report(violations: &[String]) -> String {
    let mut message = format!("\n{} module layering violation(s):\n", violations.len());
    for violation in violations {
        let _ = writeln!(message, "  - {violation}");
    }
    message
}

// --- proof that each rule fires -----------------------------------------------------------------
//
// A layering checker that silently stops matching is worse than no checker: the invariant quietly
// stops being enforced. These cases pin every rule to a concrete regression.

fn module(relative: &str, imports: &[(&str, Option<&str>)]) -> Module {
    let package = relative.rsplit_once('/').map(|(dir, _)| dir);
    Module {
        component: "loki".to_string(),
        relative: relative.to_string(),
        imports: imports
            .iter()
            .enumerate()
            .map(|(index, (written, alias))| Import {
                resolved: resolve(package, written),
                written: (*written).to_string(),
                alias: alias.map(str::to_string),
                line: index + 4,
            })
            .collect(),
    }
}

#[test]
fn a_feature_package_may_not_import_the_presentation_layer() {
    // The exact regression that put retentionWeeksDefault in describe.pkl.
    let violations = check(&[module("resources/helm.pkl", &[("../describe.pkl", None)])]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("feature package imports root module 'describe.pkl'"));
}

#[test]
fn a_feature_package_may_import_the_vocabulary_modules() {
    let violations = check(&[module("storage/types.pkl", &[("../contract.pkl", None)])]);
    assert!(violations.is_empty(), "{violations:?}");

    // context.pkl left the allow-list with the layered layout: the cluster context now reaches a
    // folder through the evaluation scope.
    let context = check(&[module("storage/types.pkl", &[("../context.pkl", None)])]);
    assert_eq!(context.len(), 1, "{context:?}");
    assert!(context[0].contains("feature package imports root module 'context.pkl'"));
}

#[test]
fn feature_packages_may_not_import_each_other() {
    let violations = check(&[module(
        "resources/validate.pkl",
        &[("../storage/backends.pkl", Some("storageBackends"))],
    )]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("from feature package 'storage'"));
}

#[test]
fn a_folder_may_read_a_sibling_setting_through_its_declaration_only() {
    let allowed = check(&[module(
        "highAvailability/dependencies.pkl",
        &[("../storage/setting.pkl", Some("storageSetting"))],
    )]);
    assert!(allowed.is_empty(), "{allowed:?}");

    let helm = check(&[module(
        "highAvailability/dependencies.pkl",
        &[("../storage/helm.pkl", Some("storageHelm"))],
    )]);
    assert_eq!(helm.len(), 1, "{helm:?}");
    assert!(helm[0].contains("only import a sibling's setting.pkl"));
}

#[test]
fn sibling_setting_imports_may_not_form_a_cycle() {
    let violations = check(&[
        module(
            "highAvailability/dependencies.pkl",
            &[("../storage/setting.pkl", Some("storageSetting"))],
        ),
        module(
            "storage/setting.pkl",
            &[("../highAvailability/setting.pkl", Some("highAvailabilitySetting"))],
        ),
    ]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("cycle"), "{violations:?}");

    let chain = check(&[
        module(
            "resources/setting.pkl",
            &[("../highAvailability/setting.pkl", Some("highAvailabilitySetting"))],
        ),
        module(
            "highAvailability/dependencies.pkl",
            &[("../storage/setting.pkl", Some("storageSetting"))],
        ),
    ]);
    assert!(chain.is_empty(), "{chain:?}");
}

#[test]
fn a_cross_package_import_must_carry_the_derived_alias() {
    // `storage` used to mean storage/types.pkl in two files and storage/backends.pkl in three.
    let violations = check(&[module("compile.pkl", &[("storage/types.pkl", Some("storage"))])]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("must be aliased 'storageTypes'"));

    let missing = check(&[module("compile.pkl", &[("storage/types.pkl", None)])]);
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert!(missing[0].contains("<no alias>"));

    let correct = check(&[module("compile.pkl", &[("storage/types.pkl", Some("storageTypes"))])]);
    assert!(correct.is_empty(), "{correct:?}");
}

#[test]
fn a_same_package_import_must_not_be_aliased() {
    let violations = check(&[module("storage/helm.pkl", &[("types.pkl", Some("storage"))])]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("same-package imports must not be aliased"));
}

#[test]
fn the_entrypoint_may_not_bypass_the_evaluation_envelope() {
    let violations = check(&[module("model.pkl", &[("compile.pkl", None)])]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("entrypoint imports 'compile.pkl'"));

    // Request decoding lives in the vendored SDK, so the entrypoint no longer parses JSON itself.
    let json = check(&[module("model.pkl", &[("pkl:json", None), ("settings.pkl", None)])]);
    assert_eq!(json.len(), 1, "{json:?}");
    assert!(json[0].contains("entrypoint imports 'pkl:json'"));

    // A component-local orchestration module is the layered layout: the SDK evaluator owns it now.
    let layered = check(&[module("model.pkl", &[("evaluation.pkl", None)])]);
    assert_eq!(layered.len(), 1, "{layered:?}");

    let correct = check(&[module(
        "model.pkl",
        &[
            ("settings.pkl", None),
            ("sdk/evaluate.pkl", Some("sdkEvaluate")),
            ("sdk/request.pkl", Some("sdkRequest")),
        ],
    )]);
    assert!(correct.is_empty(), "{correct:?}");
}

#[test]
fn the_vendored_contract_must_stay_a_leaf() {
    let violations = check(&[module("contract.pkl", &[("context.pkl", None)])]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("vendored contract imports 'context.pkl'"));

    // The stdlib is not exempt: "must import nothing" includes pkl: modules.
    let stdlib = check(&[module("contract.pkl", &[("pkl:json", None)])]);
    assert_eq!(stdlib.len(), 1, "{stdlib:?}");
    assert!(stdlib[0].contains("vendored contract imports 'pkl:json'"));
}

#[test]
fn any_module_may_import_the_vendored_sdk_with_the_derived_alias() {
    let violations = check(&[
        module("validate.pkl", &[("sdk/validate.pkl", Some("sdkValidate"))]),
        module("storage/validate.pkl", &[("../sdk/validate.pkl", Some("sdkValidate"))]),
    ]);
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn the_vendored_sdk_may_import_the_contract_but_not_component_vocabulary() {
    let correct = check(&[module(
        "sdk/request.pkl",
        &[("pkl:json", None), ("../contract.pkl", None)],
    )]);
    assert!(correct.is_empty(), "{correct:?}");

    // context.pkl is component-specific: an SDK module reaching it could no longer be vendored
    // byte-identically into a bundle that does not define it.
    let violations = check(&[module("sdk/request.pkl", &[("../context.pkl", None)])]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("the vendored SDK imports root module 'context.pkl'"));
}

#[test]
fn the_vendored_sdk_may_not_import_feature_packages() {
    let violations = check(&[module(
        "sdk/validate.pkl",
        &[("../storage/types.pkl", Some("storageTypes"))],
    )]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("the vendored SDK imports 'storage/types.pkl'"));
}

#[test]
fn import_lines_are_parsed_but_prose_is_not() {
    assert_eq!(
        parse_import(r#"import "storage/types.pkl" as storageTypes"#),
        Some(("storage/types.pkl".to_string(), Some("storageTypes".to_string())))
    );
    assert_eq!(parse_import(r#"import "pkl:json""#), Some(("pkl:json".to_string(), None)));
    assert_eq!(parse_import("// import \"describe.pkl\" would be a layering violation"), None);
    assert_eq!(
        parse_import("  import \"indented.pkl\""),
        Some(("indented.pkl".to_string(), None))
    );
}

#[test]
fn an_indented_import_cannot_bypass_the_layering_rules() {
    // Goes through parse_module on purpose: the module() helper above builds imports directly,
    // so only the real parser can prove an indented import is still seen by the checker.
    let source = "module qovery.engine.platform.loki.resources.helm\n\n  import \"../describe.pkl\"\n";
    let violations = check(&[parse_module("loki", "resources/helm.pkl", source)]);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("feature package imports root module 'describe.pkl'"));
}

#[test]
fn violations_are_reported_with_their_source_line() {
    let violations = check(&[module("resources/helm.pkl", &[("../describe.pkl", None)])]);
    assert!(violations[0].starts_with("loki/resources/helm.pkl:4:"), "{violations:?}");
}
