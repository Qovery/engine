use crate::errors::{CommandError, ErrorMessageVerbosity};
use serde_json::Value as JsonValue;

use super::govc::run_govc_command;

// ── Error detection ──────────────────────────────────────────────────────────

pub(super) fn is_tag_not_found_in_category(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    lower_error.contains("tag") && lower_error.contains("not found in category")
}

pub(super) fn is_tag_name_not_found(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    lower_error.contains("tag \"") && lower_error.contains("\" not found")
}

pub(super) fn is_inventory_object_not_found_error(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    (lower_error.contains("not found") || lower_error.contains("no such vm"))
        && (lower_error.contains("/vm/")
            || lower_error.contains("virtualmachine")
            || lower_error.contains("vm.info")
            || lower_error.contains("tags.attached.ls")
            || lower_error.contains("no such vm"))
}

pub(super) fn is_flag_not_defined_error(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    lower_error.contains("flag provided but not defined")
        || lower_error.contains("unknown flag")
        || lower_error.contains("unknown shorthand flag")
}

pub(super) fn is_tag_identifier_not_found_error(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    (lower_error.contains("/rest/com/vmware/cis/tagging/tag/id:")
        || lower_error.contains("inventoryservicetag")
        || lower_error.contains("tag/id:"))
        && lower_error.contains("404")
}

pub(super) fn is_tagging_cardinality_violation(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    lower_error.contains("tagging cardinality violation")
        || (lower_error.contains("cardinality") && lower_error.contains("violation"))
}

pub(super) fn is_tag_already_attached(error: &CommandError) -> bool {
    let lower_error =
        format!("{}\n{}", error.message_safe(), error.message_raw().unwrap_or_default()).to_ascii_lowercase();
    lower_error.contains("already attached")
}

// ── Tag name & value utilities ───────────────────────────────────────────────

pub(super) fn canonical_tag_name_for_category(category: &str, tag_value: &str) -> String {
    let normalized_value = tag_value.trim();
    if normalized_value.is_empty() {
        return String::new();
    }

    if category_requires_prefixed_tag_name(category) && !is_category_prefixed_tag_name(category, normalized_value) {
        format!("{category}:{normalized_value}")
    } else {
        normalized_value.to_string()
    }
}

pub(super) fn canonical_tag_value_for_category(category: &str, tag_value: &str) -> String {
    attached_tag_value_for_category(tag_value, category).unwrap_or_else(|| tag_value.trim().to_string())
}

pub(super) fn tag_name_candidates_for_category(category: &str, tag_value: &str) -> Vec<String> {
    let normalized_value = tag_value.trim();
    if normalized_value.is_empty() {
        return Vec::new();
    }

    let mut candidates = vec![normalized_value.to_string()];
    if let Some(unqualified_value) = attached_tag_value_for_category(normalized_value, category) {
        candidates.push(unqualified_value.clone());
        candidates.push(format!("{category}:{unqualified_value}"));
    }
    if !is_category_prefixed_tag_name(category, normalized_value) {
        candidates.push(format!("{category}:{normalized_value}"));
    }

    candidates.sort();
    candidates.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    candidates
}

pub(super) fn is_category_prefixed_tag_name(category: &str, tag_name: &str) -> bool {
    let Some((prefix, _)) = tag_name.split_once(':') else {
        return false;
    };
    prefix.trim().eq_ignore_ascii_case(category)
}

fn category_requires_prefixed_tag_name(category: &str) -> bool {
    category.eq_ignore_ascii_case("eksdRelease") || category.eq_ignore_ascii_case("os")
}

pub(super) fn attached_tag_value_for_category(attached_tag: &str, category: &str) -> Option<String> {
    let trimmed = attached_tag.trim();
    if trimmed.is_empty() {
        return None;
    }

    for separator in ['/', ':'] {
        if let Some((left, right)) = trimmed.split_once(separator) {
            let left = left.trim();
            if left.eq_ignore_ascii_case(category) {
                return normalize_attached_tag_value(right);
            }
        }
    }

    None
}

fn normalize_attached_tag_value(raw_value: &str) -> Option<String> {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return None;
    }

    // `govc tags.attached.ls -r` may append inherited/source metadata after the tag value.
    let without_annotation = trimmed.split_once(" (").map_or(trimmed, |(tag, _)| tag.trim());
    let first_token = without_annotation.split_whitespace().next().unwrap_or("").trim();
    let normalized = first_token.trim_matches(['`', '"']).trim();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

pub(super) fn attached_tag_values_for_category(attached_tags: &[String], category: &str) -> Vec<String> {
    let mut values = attached_tags
        .iter()
        .flat_map(|tag| attached_tag_candidates_for_category(tag.as_str(), category))
        .collect::<Vec<_>>();
    values.sort();
    values.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    values
}

fn attached_tag_candidates_for_category(attached_tag: &str, category: &str) -> Vec<String> {
    let mut candidates = Vec::new();

    if let Some(qualified_value) = attached_tag_value_for_category(attached_tag, category) {
        candidates.push(qualified_value);
    }

    if let Some(raw_name) = normalize_attached_tag_value(attached_tag) {
        if is_category_prefixed_tag_name(category, raw_name.as_str()) {
            if let Some((_, raw_value)) = raw_name.split_once(':')
                && let Some(unqualified_value) = normalize_attached_tag_value(raw_value)
            {
                candidates.push(unqualified_value);
            }
        } else if !raw_name.contains(':') && !raw_name.contains('/') {
            // Some govc/vCenter combinations return only the tag name without category prefix.
            candidates.push(raw_name);
        }
    }

    candidates.sort();
    candidates.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    candidates
}

pub(super) fn has_expected_eksd_release_tag(attached_tags: &[String], expected_fragment: Option<&str>) -> bool {
    attached_tag_values_for_category(attached_tags, "eksdRelease")
        .iter()
        .any(|tag_value| match expected_fragment {
            Some(fragment) => tag_value.to_lowercase().contains(fragment),
            None => true,
        })
}

pub(super) fn has_exact_eksd_release_tag(attached_tags: &[String], expected_tag: &str) -> bool {
    let normalized_expected = canonical_tag_value_for_category("eksdRelease", expected_tag);
    attached_tag_values_for_category(attached_tags, "eksdRelease")
        .iter()
        .any(|current| current.eq_ignore_ascii_case(normalized_expected.as_str()))
}

pub(super) fn has_expected_os_tag(attached_tags: &[String], expected_os_family: &str) -> bool {
    let expected_os_family = expected_os_family.to_lowercase();

    attached_tag_values_for_category(attached_tags, "os")
        .iter()
        .any(|tag_value| tag_value.to_lowercase().contains(&expected_os_family))
}

// ── Tag CRUD operations ──────────────────────────────────────────────────────

pub(super) fn ensure_tag_and_attach(
    category: &str,
    tag_value: &str,
    template_path: &str,
    govc_env: &[(String, String)],
) -> Result<(), CommandError> {
    let requested_tag_name = canonical_tag_name_for_category(category, tag_value);
    if requested_tag_name.is_empty() {
        return Err(CommandError::new_from_safe_message(format!(
            "Cannot ensure empty tag for category `{category}`"
        )));
    }
    let requested_tag_value = canonical_tag_value_for_category(category, requested_tag_name.as_str());

    if let Err(e) = run_govc_command(&["tags.category.create", "-t", "VirtualMachine", category], govc_env) {
        debug!("tags.category.create for `{category}`: {e} (likely already exists)");
    }
    if let Err(e) = run_govc_command(&["tags.create", "-c", category, requested_tag_name.as_str()], govc_env) {
        debug!("tags.create `{requested_tag_name}`: {e} (likely already exists)");
    }

    let attached_tags = run_govc_command(&["tags.attached.ls", "-r", template_path], govc_env)?;
    let mut expected_tag_seen_in_recursive_listing = false;
    for attached_tag in attached_tags {
        let Some(attached_tag_value) = attached_tag_value_for_category(attached_tag.as_str(), category) else {
            continue;
        };

        if attached_tag_value.eq_ignore_ascii_case(requested_tag_value.as_str()) {
            expected_tag_seen_in_recursive_listing = true;
            continue;
        }

        let mut detached = false;
        for tag_candidate in tag_name_candidates_for_category(category, attached_tag_value.as_str()) {
            match run_govc_command(
                &["tags.detach", "-c", category, tag_candidate.as_str(), template_path],
                govc_env,
            ) {
                Ok(_) => {
                    detached = true;
                    break;
                }
                Err(detach_error)
                    if is_tag_not_found_in_category(&detach_error) || is_tag_name_not_found(&detach_error) =>
                {
                    continue;
                }
                Err(detach_error) => return Err(detach_error),
            }
        }
        if !detached {
            debug!(
                "Could not detach `{}` on `{}` with category `{}` using known tag-name variants",
                attached_tag_value, template_path, category
            );
        }
    }

    if expected_tag_seen_in_recursive_listing
        && is_tag_directly_attached_to_object(category, requested_tag_name.as_str(), template_path, govc_env)?
    {
        return Ok(());
    }

    if let Err(attach_error) = run_govc_command(
        &[
            "tags.attach",
            "-c",
            category,
            requested_tag_name.as_str(),
            template_path,
        ],
        govc_env,
    ) {
        if is_tag_already_attached(&attach_error) {
            return Ok(());
        }

        if is_tagging_cardinality_violation(&attach_error) {
            let recursive_attached_tags =
                run_govc_command(&["tags.attached.ls", "-r", template_path], govc_env).unwrap_or_default();
            let conflicting_tags = attached_tag_values_for_category(&recursive_attached_tags, category)
                .into_iter()
                .filter(|current| !current.eq_ignore_ascii_case(requested_tag_value.as_str()))
                .collect::<Vec<_>>();

            // Best effort: resolve inherited conflicts by detaching the old category tag from
            // ancestor objects that own it, then retry attaching the expected tag on the template.
            let mut detached_objects = Vec::new();
            for conflicting_tag in &conflicting_tags {
                detached_objects.extend(detach_conflicting_tag_from_template_ancestors(
                    category,
                    conflicting_tag,
                    template_path,
                    govc_env,
                )?);
            }

            match run_govc_command(
                &[
                    "tags.attach",
                    "-c",
                    category,
                    requested_tag_name.as_str(),
                    template_path,
                ],
                govc_env,
            ) {
                Ok(_) => return Ok(()),
                Err(retry_error) if is_tag_already_attached(&retry_error) => return Ok(()),
                Err(retry_error) => {
                    let conflicts_display = if conflicting_tags.is_empty() {
                        format!("category `{category}` already has a conflicting attached or inherited tag")
                    } else {
                        format!("conflicting tag(s): `{}`", conflicting_tags.join("`, `"))
                    };
                    let detached_display = if detached_objects.is_empty() {
                        "no ancestor object could be detached automatically".to_string()
                    } else {
                        format!("detached conflicting tags from: `{}`", detached_objects.join("`, `"))
                    };

                    return Err(CommandError::new(
                        format!(
                            "Cannot attach `{requested_tag_name}` to `{template_path}` due to vSphere tag cardinality. {conflicts_display}. \
Automatic remediation attempted ({detached_display}) but attach still failed."
                        ),
                        Some(retry_error.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                        None,
                    ));
                }
            }
        }

        return Err(attach_error);
    }

    Ok(())
}

fn detach_conflicting_tag_from_template_ancestors(
    category: &str,
    conflicting_tag: &str,
    template_path: &str,
    govc_env: &[(String, String)],
) -> Result<Vec<String>, CommandError> {
    let attached_objects = list_objects_attached_to_tag(category, conflicting_tag, govc_env)?;
    let mut detached_objects = Vec::new();

    for object_path in attached_objects {
        if !is_same_or_ancestor_inventory_path(template_path, object_path.as_str()) {
            continue;
        }

        let mut detached = false;
        for tag_candidate in tag_name_candidates_for_category(category, conflicting_tag) {
            match run_govc_command(
                &[
                    "tags.detach",
                    "-c",
                    category,
                    tag_candidate.as_str(),
                    object_path.as_str(),
                ],
                govc_env,
            ) {
                Ok(_) => {
                    detached = true;
                    break;
                }
                Err(detach_error)
                    if is_tag_not_found_in_category(&detach_error) || is_tag_name_not_found(&detach_error) =>
                {
                    continue;
                }
                Err(detach_error) => return Err(detach_error),
            }
        }
        if detached {
            detached_objects.push(object_path);
        }
    }

    if detached_objects.is_empty() {
        // Fallback for older/inconsistent tagging APIs: try detaching on each ancestor path.
        for candidate in inventory_path_ancestors(template_path) {
            let mut detached = false;
            for tag_candidate in tag_name_candidates_for_category(category, conflicting_tag) {
                match run_govc_command(
                    &[
                        "tags.detach",
                        "-c",
                        category,
                        tag_candidate.as_str(),
                        candidate.as_str(),
                    ],
                    govc_env,
                ) {
                    Ok(_) => {
                        detached = true;
                        break;
                    }
                    Err(detach_error)
                        if is_tag_not_found_in_category(&detach_error) || is_tag_name_not_found(&detach_error) =>
                    {
                        continue;
                    }
                    Err(detach_error) => return Err(detach_error),
                }
            }
            if detached {
                detached_objects.push(candidate);
            }
        }
        detached_objects.sort();
        detached_objects.dedup();
    }

    Ok(detached_objects)
}

fn list_objects_attached_to_tag(
    category: &str,
    tag_value: &str,
    govc_env: &[(String, String)],
) -> Result<Vec<String>, CommandError> {
    Ok(normalize_inventory_object_paths(&list_attached_entries_for_tag(
        category, tag_value, govc_env,
    )?))
}

fn list_attached_entries_for_tag(
    category: &str,
    tag_value: &str,
    govc_env: &[(String, String)],
) -> Result<Vec<String>, CommandError> {
    let tag_candidates = tag_name_candidates_for_category(category, tag_value);
    let mut last_error = None;
    debug!(
        "Resolving attached objects for tag category=`{}` value=`{}` candidates={:?}",
        category, tag_value, tag_candidates
    );

    for candidate in &tag_candidates {
        match run_govc_command(&["tags.attached.ls", "-c", category, candidate.as_str()], govc_env) {
            Ok(output) => {
                debug!(
                    "Resolved attached objects with `tags.attached.ls -c`: candidate=`{}` output={:?}",
                    candidate, output
                );
                return Ok(output);
            }
            Err(err) if is_tag_not_found_in_category(&err) || is_tag_name_not_found(&err) => {
                last_error = Some(err);
                continue;
            }
            Err(err) if is_flag_not_defined_error(&err) => {
                // Older govc versions do not support `tags.attached.ls -c`; fall through to identifier lookup.
                last_error = Some(err);
                break;
            }
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        }
    }

    let mut identifiers = tag_candidates.clone();
    for candidate in tag_candidates {
        if let Some(tag_id) = resolve_tag_id_for_category(category, candidate.as_str(), govc_env)?
            && !identifiers
                .iter()
                .any(|identifier| identifier.eq_ignore_ascii_case(tag_id.as_str()))
        {
            identifiers.push(tag_id);
        }
    }

    for identifier in identifiers {
        match run_govc_command(&["tags.attached.ls", identifier.as_str()], govc_env) {
            Ok(output) => {
                debug!(
                    "Resolved attached objects with `tags.attached.ls` identifier=`{}` output={:?}",
                    identifier, output
                );
                return Ok(output);
            }
            Err(err) if is_tag_identifier_not_found_error(&err) => {
                last_error = Some(err);
                continue;
            }
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        }
    }

    if let Some(err) = last_error {
        if is_tag_identifier_not_found_error(&err) || is_tag_not_found_in_category(&err) || is_tag_name_not_found(&err)
        {
            return Ok(Vec::new());
        }

        return Err(err);
    }

    Ok(Vec::new())
}

pub(super) fn is_tag_directly_attached_to_object(
    category: &str,
    tag_value: &str,
    object_path: &str,
    govc_env: &[(String, String)],
) -> Result<bool, CommandError> {
    let attached_entries = list_attached_entries_for_tag(category, tag_value, govc_env)?;
    let vm_moid = vm_moid_for_template(object_path, govc_env)?;
    let is_attached = attached_entries.iter().any(|entry| {
        normalize_inventory_object_path(entry.as_str())
            .is_some_and(|path| is_same_inventory_path(path.as_str(), object_path))
            || vm_moid
                .as_deref()
                .is_some_and(|moid| is_dynamic_vm_reference_to_moid(entry.as_str(), moid))
    });
    debug!(
        "Direct tag attachment check: category=`{}` tag=`{}` object=`{}` vm_moid={:?} attached_entries={:?} result={}",
        category, tag_value, object_path, vm_moid, attached_entries, is_attached
    );
    Ok(is_attached)
}

// ── Inventory path utilities ─────────────────────────────────────────────────

pub(super) fn normalize_inventory_object_path(raw_line: &str) -> Option<String> {
    let trimmed = raw_line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let start_index = trimmed.find('/')?;
    let path_candidate = trimmed[start_index..]
        .split_once(" (")
        .map_or(&trimmed[start_index..], |(path, _)| path)
        .trim();
    let normalized = path_candidate.trim_matches(['`', '"']);

    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn normalize_inventory_object_paths(output_lines: &[String]) -> Vec<String> {
    output_lines
        .iter()
        .filter_map(|line| normalize_inventory_object_path(line.as_str()))
        .collect::<Vec<_>>()
}

pub(super) fn is_same_or_ancestor_inventory_path(descendant_path: &str, ancestor_path: &str) -> bool {
    let descendant = descendant_path.trim_end_matches('/');
    let ancestor = ancestor_path.trim_end_matches('/');

    descendant == ancestor || descendant.starts_with(format!("{ancestor}/").as_str())
}

fn is_same_inventory_path(left: &str, right: &str) -> bool {
    left.trim_end_matches('/')
        .eq_ignore_ascii_case(right.trim_end_matches('/'))
}

fn inventory_path_ancestors(path: &str) -> Vec<String> {
    let mut normalized = path.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() || !normalized.starts_with('/') {
        return Vec::new();
    }

    let mut ancestors = Vec::new();
    loop {
        ancestors.push(normalized.clone());
        if let Some((parent, _)) = normalized.rsplit_once('/') {
            if parent.is_empty() {
                break;
            }
            normalized = parent.to_string();
        } else {
            break;
        }
    }

    ancestors
}

// ── VM MoID resolution ───────────────────────────────────────────────────────

fn vm_moid_for_template(template_path: &str, govc_env: &[(String, String)]) -> Result<Option<String>, CommandError> {
    let vm_info_output = run_govc_command(&["vm.info", "-json", template_path], govc_env)?;
    let vm_info_json = vm_info_output.join("\n");
    let vm_info: JsonValue = serde_json::from_str(vm_info_json.as_str()).map_err(|e| {
        CommandError::new(
            format!("Cannot parse `govc vm.info -json` output for `{template_path}`"),
            Some(e.to_string()),
            None,
        )
    })?;
    let structured_moid = vm_info
        .get("VirtualMachines")
        .and_then(JsonValue::as_array)
        .and_then(|vms| vms.first())
        .and_then(|vm| vm.get("Self"))
        .and_then(|self_ref| self_ref.get("Value"))
        .and_then(JsonValue::as_str)
        .and_then(normalize_vm_moid);
    let fallback_moid = extract_vm_moid_from_vm_info_json(vm_info_json.as_str());
    let final_moid = structured_moid.or(fallback_moid);
    debug!("Resolved VM MoID for template `{}`: {:?}", template_path, final_moid);
    Ok(final_moid)
}

pub(super) fn normalize_vm_moid(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = trimmed.split(':').next().unwrap_or(trimmed).trim();
    if candidate.starts_with("vm-") {
        Some(candidate.to_string())
    } else {
        None
    }
}

pub(super) fn extract_vm_moid_from_vm_info_json(vm_info_json: &str) -> Option<String> {
    for (idx, ch) in vm_info_json.char_indices() {
        if ch != 'v' {
            continue;
        }

        let remaining = &vm_info_json[idx..];
        if !remaining.starts_with("vm-") {
            continue;
        }

        let mut end = idx + 3;
        let mut has_digit = false;
        for (offset, c) in vm_info_json[(idx + 3)..].char_indices() {
            if c.is_ascii_digit() {
                has_digit = true;
                end = idx + 3 + offset + c.len_utf8();
                continue;
            }
            break;
        }

        if has_digit {
            return Some(vm_info_json[idx..end].to_string());
        }
    }

    None
}

fn is_dynamic_vm_reference_to_moid(entry: &str, moid: &str) -> bool {
    let trimmed = entry.trim();
    if trimmed.is_empty() || moid.trim().is_empty() {
        return false;
    }

    let lower_entry = trimmed.to_ascii_lowercase();
    let lower_moid = moid.trim().to_ascii_lowercase();

    let dynamic_prefix = format!("virtualmachine:{lower_moid}");
    if lower_entry == dynamic_prefix || lower_entry.starts_with(format!("{dynamic_prefix}:").as_str()) {
        return true;
    }

    lower_entry.contains(format!("id = {lower_moid}").as_str())
        || lower_entry.contains(format!("id={lower_moid}").as_str())
        || lower_entry.contains(format!("id = {lower_moid}:").as_str())
        || lower_entry.contains(format!("id={lower_moid}:").as_str())
}

// ── Tag ID resolution ────────────────────────────────────────────────────────

fn resolve_tag_id_for_category(
    category: &str,
    tag_value: &str,
    govc_env: &[(String, String)],
) -> Result<Option<String>, CommandError> {
    let output = match run_govc_command(&["tags.info", "-c", category, tag_value], govc_env) {
        Ok(output) => output,
        Err(err)
            if is_tag_not_found_in_category(&err)
                || is_tag_name_not_found(&err)
                || is_tag_identifier_not_found_error(&err) =>
        {
            return Ok(None);
        }
        Err(err) => return Err(err),
    };

    Ok(parse_tag_id_from_tags_info_output(&output))
}

pub(super) fn parse_tag_id_from_tags_info_output(output_lines: &[String]) -> Option<String> {
    output_lines.iter().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if !key.trim().eq_ignore_ascii_case("id") {
            return None;
        }

        let id = value.trim();
        if id.is_empty() { None } else { Some(id.to_string()) }
    })
}
