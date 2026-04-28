use crate::types::Rule;

/// Returns the highest-priority rule that matches `file_path` for the given `package_type`.
///
/// Lower `rule.priority` value wins. Returns `None` if no rule matches.
pub fn best_match<'a>(
    rules: &'a [Rule],
    package_type: &str,
    file_path: &str,
) -> Option<&'a Rule> {
    let mut candidates: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.r#type == package_type)
        .filter(|r| glob::Pattern::new(&r.r#match).map_or(false, |p| p.matches(file_path)))
        .collect();

    candidates.sort_by_key(|r| r.priority);
    candidates.into_iter().next()
}
