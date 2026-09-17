/// `textDocument/prepareTypeHierarchy`, `typeHierarchy/supertypes`, `typeHierarchy/subtypes`.
use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Value, json};
use tower_lsp_server::ls_types::{SymbolKind, TypeHierarchyItem, Uri};

use crate::document::ast::ParsedDoc;
use crate::text::zero_width_range;

fn make_item_from_index(
    name: &str,
    kind: SymbolKind,
    uri: &Uri,
    start_line: u32,
    fqn: &str,
) -> TypeHierarchyItem {
    let range = zero_width_range(start_line);
    TypeHierarchyItem {
        name: name.to_string(),
        kind,
        tags: None,
        detail: None,
        uri: uri.clone(),
        range,
        selection_range: range,
        data: Some(json!({ "fqn": fqn.trim_start_matches('\\') })),
    }
}

fn sort_items_stably(items: &mut [TypeHierarchyItem]) {
    items.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.uri.as_str().cmp(b.uri.as_str()))
            .then_with(|| a.range.start.line.cmp(&b.range.start.line))
            .then_with(|| a.range.start.character.cmp(&b.range.start.character))
    });
}

pub fn item_fqn(item: &TypeHierarchyItem) -> Option<&str> {
    item.data
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("fqn"))
        .and_then(Value::as_str)
}

/// Phase J — Prepare from the salsa-memoized workspace aggregate.
/// Build a hierarchy item from its canonical FQN.
pub fn prepare_type_hierarchy_from_fqn(
    class_ref: crate::db::workspace_index::ClassRef,
    wi: &crate::db::workspace_index::WorkspaceIndexData,
) -> Option<TypeHierarchyItem> {
    use crate::index::file_index::ClassKind;
    let (uri, cls) = wi.at(class_ref)?;
    let kind = match cls.kind {
        ClassKind::Class | ClassKind::Trait => SymbolKind::CLASS,
        ClassKind::Interface => SymbolKind::INTERFACE,
        ClassKind::Enum => SymbolKind::ENUM,
    };
    Some(make_item_from_index(
        &cls.name,
        kind,
        uri,
        cls.start_line,
        &cls.fqn,
    ))
}

/// Supertypes via the canonical FQN carried in the hierarchy item.
pub fn supertypes_of_from_workspace(
    item: &TypeHierarchyItem,
    wi: &crate::db::workspace_index::WorkspaceIndexData,
    get_doc: &dyn Fn(&Uri) -> Option<Arc<ParsedDoc>>,
    resolve_class_ref: &dyn Fn(&str) -> Option<crate::db::workspace_index::ClassRef>,
) -> Vec<TypeHierarchyItem> {
    use crate::index::file_index::ClassKind;
    let mut result = Vec::new();
    let mut seen_fqns: HashSet<Box<str>> = HashSet::new();
    let Some(item_fqn) = item_fqn(item) else {
        return result;
    };
    let Some(class_ref) = resolve_class_ref(item_fqn) else {
        return result;
    };
    let Some((uri, cls)) = wi.at(class_ref) else {
        return result;
    };
    let Some(doc) = get_doc(uri) else {
        return result;
    };
    let imports = doc.file_imports();
    let super_names = cls
        .parent
        .iter()
        .cloned()
        .chain(cls.implements.iter().cloned())
        .chain(cls.traits.iter().cloned());
    for name in super_names {
        let resolved = crate::navigation::moniker::resolve_fqn(&doc, name.as_ref(), &imports);
        let Some((super_uri, super_cls)) =
            resolve_class_ref(&resolved).and_then(|class_ref| wi.at(class_ref))
        else {
            continue;
        };
        if seen_fqns.insert(super_cls.fqn.clone()) {
            let kind = match super_cls.kind {
                ClassKind::Class | ClassKind::Trait => SymbolKind::CLASS,
                ClassKind::Interface => SymbolKind::INTERFACE,
                ClassKind::Enum => SymbolKind::ENUM,
            };
            result.push(make_item_from_index(
                &super_cls.name,
                kind,
                super_uri,
                super_cls.start_line,
                &super_cls.fqn,
            ));
        }
    }
    result
}

/// Mir-backed variant of [`subtypes_of_from_workspace`].
///
/// `item_fqn` is the FQCN of the hierarchy item (e.g. `"App\\Animal"`),
/// resolved in the handler from the workspace index. `subtype_urls` is the
/// file set from `DocumentStore::class_subtype_urls`. When non-empty this
/// fixes aliased `extends` and FQN-qualified forms the raw-name map misses.
/// Falls back to [`subtypes_of_from_workspace`] when `subtype_urls` is empty.
pub fn subtypes_of_mir_backed(
    item: &TypeHierarchyItem,
    item_fqn: &str,
    wi: &crate::db::workspace_index::WorkspaceIndexData,
    subtype_urls: &[Uri],
    mention_candidates: &dyn Fn(&str) -> Vec<Uri>,
    get_doc: &dyn Fn(&Uri) -> Option<Arc<ParsedDoc>>,
) -> Vec<TypeHierarchyItem> {
    if subtype_urls.is_empty() {
        return subtypes_of_from_workspace(item, item_fqn, wi, mention_candidates, get_doc);
    }
    use crate::index::file_index::ClassKind;
    let mut result = Vec::new();
    wi.for_each_class_in_uris(subtype_urls, |uri, cls| {
        let doc = get_doc(uri);
        let imports = doc.as_ref().map(|doc| doc.file_imports());
        let matches_name = |name: &str| {
            if let (Some(doc), Some(imports)) = (doc.as_ref(), imports.as_ref()) {
                crate::navigation::moniker::resolve_fqn(doc, name, imports)
                    .trim_start_matches('\\')
                    .eq_ignore_ascii_case(item_fqn)
            } else {
                false
            }
        };
        let extends_match = cls.parent.as_deref().is_some_and(matches_name);
        let implements_match = cls
            .implements
            .iter()
            .any(|iface| matches_name(iface.as_ref()));
        let uses_match = cls.traits.iter().any(|t| matches_name(t.as_ref()));
        if extends_match || implements_match || uses_match {
            let kind = match cls.kind {
                ClassKind::Class | ClassKind::Trait => SymbolKind::CLASS,
                ClassKind::Interface => SymbolKind::INTERFACE,
                ClassKind::Enum => SymbolKind::ENUM,
            };
            result.push(make_item_from_index(
                &cls.name,
                kind,
                uri,
                cls.start_line,
                &cls.fqn,
            ));
        }
    });
    sort_items_stably(&mut result);
    result
}

/// Phase J — Subtypes via mir's mention index: files that mention
/// `item.name` at all are the only ones whose `extends`/`implements`/`use`
/// clause could possibly name it, so mention-candidates replaces the old
/// eagerly-rebuilt `subtypes_of` reverse map as the narrowing step.
///
/// `item_fqn` is the canonical FQCN of the hierarchy item. A mention hit is
/// necessary but not sufficient (over-inclusive across a large workspace,
/// e.g. many unrelated `Factory` interfaces each aliased to the same
/// `FactoryContract` locally), so each candidate is re-checked against
/// `item_fqn` via `resolves_to_fqn`, which resolves the candidate's own
/// `extends`/`implements`/`use` clause through its `use_imports` and
/// namespace before accepting the match.
pub fn subtypes_of_from_workspace(
    item: &TypeHierarchyItem,
    item_fqn: &str,
    wi: &crate::db::workspace_index::WorkspaceIndexData,
    mention_candidates: &dyn Fn(&str) -> Vec<Uri>,
    get_doc: &dyn Fn(&Uri) -> Option<Arc<ParsedDoc>>,
) -> Vec<TypeHierarchyItem> {
    use crate::index::file_index::ClassKind;
    let mut results = Vec::new();
    let candidate_uris = mention_candidates(&item.name);
    wi.for_each_class_in_uris(&candidate_uris, |uri, cls| {
        let doc = get_doc(uri);
        let imports = doc.as_ref().map(|doc| doc.file_imports());
        let named = |name: &str| {
            let (Some(doc), Some(imports)) = (doc.as_ref(), imports.as_ref()) else {
                return false;
            };
            crate::navigation::moniker::resolve_fqn(doc, name, imports)
                .trim_start_matches('\\')
                .eq_ignore_ascii_case(item_fqn)
        };
        let matches = cls.parent.as_deref().is_some_and(named)
            || cls.implements.iter().any(|iface| named(iface.as_ref()))
            || cls.traits.iter().any(|t| named(t.as_ref()));
        if matches {
            let kind = match cls.kind {
                ClassKind::Class | ClassKind::Trait => SymbolKind::CLASS,
                ClassKind::Interface => SymbolKind::INTERFACE,
                ClassKind::Enum => SymbolKind::ENUM,
            };
            results.push(make_item_from_index(
                &cls.name,
                kind,
                uri,
                cls.start_line,
                &cls.fqn,
            ));
        }
    });
    sort_items_stably(&mut results);
    results
}
