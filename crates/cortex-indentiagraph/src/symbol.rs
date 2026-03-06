//! Symbol CRUD operations for IndentiaGraphStore.
//!
//! Covers: Function, Struct, Trait, Enum, Impl, Import — single + batch upserts.
//!
//! NOTE: Table names `function`, `struct`, `trait`, `enum`, `impl`, `import` are
//! SurrealDB reserved keywords. We use `type::record('table', $key)` for record IDs
//! and backtick-escaping for table names in queries.

use crate::client::IndentiaGraphStore;
use anyhow::{Context, Result};
use cortex_core::models::{
    EnumNode, FunctionNode, FunctionSummaryNode, ImplBlockDetailNode, ImplNode, ImportNode,
    Parameter, StructNode, StructSummaryNode, TraitImplementorNode, TraitInfoNode, TraitNode,
    TypeTraitInfoNode, Visibility,
};
use surrealdb::types::{RecordId, SurrealValue};
use uuid::Uuid;

// ============================================================================
// Helpers
// ============================================================================

fn symbol_key(file_path: &str, name: &str, line: u32) -> String {
    format!("{}::{}::{}", file_path, name, line)
}

fn impl_key(file_path: &str, for_type: &str, line: u32) -> String {
    format!("{}::{}::{}", file_path, for_type, line)
}

fn import_key(file_path: &str, path: &str, line: u32) -> String {
    format!("{}::{}::{}", file_path, path, line)
}

fn vis_to_string(v: &Visibility) -> String {
    match v {
        Visibility::Public => "public".to_string(),
        Visibility::Private => "private".to_string(),
        Visibility::Crate => "crate".to_string(),
        Visibility::Super => "super".to_string(),
        Visibility::InPath(p) => format!("in({})", p),
    }
}

pub(crate) fn string_to_vis_pub(s: &str) -> Visibility {
    string_to_vis(s)
}

fn string_to_vis(s: &str) -> Visibility {
    match s {
        "public" => Visibility::Public,
        "private" => Visibility::Private,
        "crate" => Visibility::Crate,
        "super" => Visibility::Super,
        s if s.starts_with("in(") => Visibility::InPath(
            s.trim_start_matches("in(")
                .trim_end_matches(')')
                .to_string(),
        ),
        _ => Visibility::Private,
    }
}

// ============================================================================
// SurrealDB record types for deserialization
// ============================================================================

#[derive(Debug, SurrealValue)]
struct NameRecord {
    name: String,
}

#[derive(Debug, SurrealValue)]
struct ForTypeRecord {
    for_type: String,
}

#[derive(Debug, SurrealValue)]
struct TraitNameFieldRecord {
    trait_name: Option<String>,
}

#[derive(Debug, SurrealValue)]
struct FuncSummaryRecord {
    name: String,
    line_start: i64,
    is_async: bool,
    visibility: String,
    complexity: Option<i64>,
    docstring: Option<String>,
    return_type: Option<String>,
    parameters: Option<String>,
}

#[derive(Debug, SurrealValue)]
struct StructSummaryRecord {
    name: String,
    line_start: i64,
    visibility: String,
    docstring: Option<String>,
}

#[derive(Debug, SurrealValue)]
struct TraitInfoRecord {
    is_external: bool,
    source: Option<String>,
}

#[derive(Debug, SurrealValue)]
struct ImplDetailRecord {
    for_type: String,
    file_path: String,
    line_start: i64,
    line_end: i64,
    trait_name: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, SurrealValue)]
struct ImportPathRecord {
    path: String,
}

#[derive(Debug, SurrealValue)]
struct CountRecord {
    count: i64,
}

impl IndentiaGraphStore {
    // ========================================================================
    // Single symbol upserts — use type::record() for reserved keyword tables
    // ========================================================================

    /// Upsert a function node.
    pub async fn upsert_function(&self, func: &FunctionNode) -> Result<()> {
        let key = symbol_key(&func.file_path, &func.name, func.line_start);
        self.db
            .query(
                "UPSERT type::record('function', $key) SET \
                 name = $name, visibility = $vis, parameters = $params, \
                 return_type = $ret, generics = $generics, \
                 is_async = $is_async, is_unsafe = $is_unsafe, \
                 complexity = $complexity, file_path = $fp, \
                 line_start = $ls, line_end = $le, docstring = $doc \
                 RETURN NONE",
            )
            .bind(("key", key))
            .bind(("name", func.name.clone()))
            .bind(("vis", vis_to_string(&func.visibility)))
            .bind(("params", serde_json::to_string(&func.params).unwrap()))
            .bind(("ret", func.return_type.clone()))
            .bind(("generics", serde_json::to_string(&func.generics).unwrap()))
            .bind(("is_async", func.is_async))
            .bind(("is_unsafe", func.is_unsafe))
            .bind(("complexity", func.complexity as i64))
            .bind(("fp", func.file_path.clone()))
            .bind(("ls", func.line_start as i64))
            .bind(("le", func.line_end as i64))
            .bind(("doc", func.docstring.clone()))
            .await
            .context("Failed to upsert function")?;
        Ok(())
    }

    /// Upsert a struct node.
    pub async fn upsert_struct(&self, s: &StructNode) -> Result<()> {
        let key = symbol_key(&s.file_path, &s.name, s.line_start);
        self.db
            .query(
                "UPSERT type::record('struct', $key) SET \
                 name = $name, visibility = $vis, generics = $generics, \
                 file_path = $fp, line_start = $ls, line_end = $le, \
                 docstring = $doc, parent_class = $pc, interfaces = $ifaces \
                 RETURN NONE",
            )
            .bind(("key", key))
            .bind(("name", s.name.clone()))
            .bind(("vis", vis_to_string(&s.visibility)))
            .bind(("generics", serde_json::to_string(&s.generics).unwrap()))
            .bind(("fp", s.file_path.clone()))
            .bind(("ls", s.line_start as i64))
            .bind(("le", s.line_end as i64))
            .bind(("doc", s.docstring.clone()))
            .bind(("pc", s.parent_class.clone()))
            .bind(("ifaces", serde_json::to_string(&s.interfaces).unwrap()))
            .await
            .context("Failed to upsert struct")?;
        Ok(())
    }

    /// Upsert a trait node.
    pub async fn upsert_trait(&self, t: &TraitNode) -> Result<()> {
        let key = symbol_key(&t.file_path, &t.name, t.line_start);
        self.db
            .query(
                "UPSERT type::record('trait', $key) SET \
                 name = $name, visibility = $vis, generics = $generics, \
                 file_path = $fp, line_start = $ls, line_end = $le, \
                 docstring = $doc, is_external = $ext, source = $src \
                 RETURN NONE",
            )
            .bind(("key", key))
            .bind(("name", t.name.clone()))
            .bind(("vis", vis_to_string(&t.visibility)))
            .bind(("generics", serde_json::to_string(&t.generics).unwrap()))
            .bind(("fp", t.file_path.clone()))
            .bind(("ls", t.line_start as i64))
            .bind(("le", t.line_end as i64))
            .bind(("doc", t.docstring.clone()))
            .bind(("ext", t.is_external))
            .bind(("src", t.source.clone()))
            .await
            .context("Failed to upsert trait")?;
        Ok(())
    }

    /// Upsert an enum node.
    pub async fn upsert_enum(&self, e: &EnumNode) -> Result<()> {
        let key = symbol_key(&e.file_path, &e.name, e.line_start);
        self.db
            .query(
                "UPSERT type::record('enum', $key) SET \
                 name = $name, visibility = $vis, variants = $variants, \
                 file_path = $fp, line_start = $ls, line_end = $le, docstring = $doc \
                 RETURN NONE",
            )
            .bind(("key", key))
            .bind(("name", e.name.clone()))
            .bind(("vis", vis_to_string(&e.visibility)))
            .bind(("variants", serde_json::to_string(&e.variants).unwrap()))
            .bind(("fp", e.file_path.clone()))
            .bind(("ls", e.line_start as i64))
            .bind(("le", e.line_end as i64))
            .bind(("doc", e.docstring.clone()))
            .await
            .context("Failed to upsert enum")?;
        Ok(())
    }

    /// Upsert an impl node.
    pub async fn upsert_impl(&self, imp: &ImplNode) -> Result<()> {
        let key = impl_key(&imp.file_path, &imp.for_type, imp.line_start);
        self.db
            .query(
                "UPSERT type::record('impl', $key) SET \
                 for_type = $ft, trait_name = $tn, generics = $generics, \
                 where_clause = $wc, file_path = $fp, \
                 line_start = $ls, line_end = $le \
                 RETURN NONE",
            )
            .bind(("key", key))
            .bind(("ft", imp.for_type.clone()))
            .bind(("tn", imp.trait_name.clone()))
            .bind(("generics", serde_json::to_string(&imp.generics).unwrap()))
            .bind(("wc", imp.where_clause.clone()))
            .bind(("fp", imp.file_path.clone()))
            .bind(("ls", imp.line_start as i64))
            .bind(("le", imp.line_end as i64))
            .await
            .context("Failed to upsert impl")?;
        Ok(())
    }

    /// Upsert an import node.
    pub async fn upsert_import(&self, import: &ImportNode) -> Result<()> {
        let key = import_key(&import.file_path, &import.path, import.line);
        self.db
            .query(
                "UPSERT type::record('import', $key) SET \
                 path = $path, alias = $alias, items = $items, \
                 file_path = $fp, line = $line \
                 RETURN NONE",
            )
            .bind(("key", key))
            .bind(("path", import.path.clone()))
            .bind(("alias", import.alias.clone()))
            .bind(("items", serde_json::to_string(&import.items).unwrap()))
            .bind(("fp", import.file_path.clone()))
            .bind(("line", import.line as i64))
            .await
            .context("Failed to upsert import")?;
        Ok(())
    }

    // ========================================================================
    // Batch symbol upserts
    // ========================================================================

    /// Batch upsert functions.
    pub async fn batch_upsert_functions(&self, functions: &[FunctionNode]) -> Result<()> {
        for chunk in functions.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPSERT type::record('function', $key_{i}) SET \
                     name = $name_{i}, visibility = $vis_{i}, parameters = $params_{i}, \
                     return_type = $ret_{i}, generics = $gen_{i}, \
                     is_async = $async_{i}, is_unsafe = $unsafe_{i}, \
                     complexity = $cx_{i}, file_path = $fp_{i}, \
                     line_start = $ls_{i}, line_end = $le_{i}, docstring = $doc_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, func) in chunk.iter().enumerate() {
                let key = symbol_key(&func.file_path, &func.name, func.line_start);
                q = q
                    .bind((format!("key_{i}"), key))
                    .bind((format!("name_{i}"), func.name.clone()))
                    .bind((format!("vis_{i}"), vis_to_string(&func.visibility)))
                    .bind((
                        format!("params_{i}"),
                        serde_json::to_string(&func.params).unwrap(),
                    ))
                    .bind((format!("ret_{i}"), func.return_type.clone()))
                    .bind((
                        format!("gen_{i}"),
                        serde_json::to_string(&func.generics).unwrap(),
                    ))
                    .bind((format!("async_{i}"), func.is_async))
                    .bind((format!("unsafe_{i}"), func.is_unsafe))
                    .bind((format!("cx_{i}"), func.complexity as i64))
                    .bind((format!("fp_{i}"), func.file_path.clone()))
                    .bind((format!("ls_{i}"), func.line_start as i64))
                    .bind((format!("le_{i}"), func.line_end as i64))
                    .bind((format!("doc_{i}"), func.docstring.clone()));
            }
            q.await.context("Failed to batch upsert functions")?;
        }
        Ok(())
    }

    /// Batch upsert structs.
    pub async fn batch_upsert_structs(&self, structs: &[StructNode]) -> Result<()> {
        for chunk in structs.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPSERT type::record('struct', $key_{i}) SET \
                     name = $name_{i}, visibility = $vis_{i}, generics = $gen_{i}, \
                     file_path = $fp_{i}, line_start = $ls_{i}, line_end = $le_{i}, \
                     docstring = $doc_{i}, parent_class = $pc_{i}, interfaces = $ifaces_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, s) in chunk.iter().enumerate() {
                let key = symbol_key(&s.file_path, &s.name, s.line_start);
                q = q
                    .bind((format!("key_{i}"), key))
                    .bind((format!("name_{i}"), s.name.clone()))
                    .bind((format!("vis_{i}"), vis_to_string(&s.visibility)))
                    .bind((
                        format!("gen_{i}"),
                        serde_json::to_string(&s.generics).unwrap(),
                    ))
                    .bind((format!("fp_{i}"), s.file_path.clone()))
                    .bind((format!("ls_{i}"), s.line_start as i64))
                    .bind((format!("le_{i}"), s.line_end as i64))
                    .bind((format!("doc_{i}"), s.docstring.clone()))
                    .bind((format!("pc_{i}"), s.parent_class.clone()))
                    .bind((
                        format!("ifaces_{i}"),
                        serde_json::to_string(&s.interfaces).unwrap(),
                    ));
            }
            q.await.context("Failed to batch upsert structs")?;
        }
        Ok(())
    }

    /// Batch upsert traits.
    pub async fn batch_upsert_traits(&self, traits: &[TraitNode]) -> Result<()> {
        for chunk in traits.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPSERT type::record('trait', $key_{i}) SET \
                     name = $name_{i}, visibility = $vis_{i}, generics = $gen_{i}, \
                     file_path = $fp_{i}, line_start = $ls_{i}, line_end = $le_{i}, \
                     docstring = $doc_{i}, is_external = $ext_{i}, source = $src_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, t) in chunk.iter().enumerate() {
                let key = symbol_key(&t.file_path, &t.name, t.line_start);
                q = q
                    .bind((format!("key_{i}"), key))
                    .bind((format!("name_{i}"), t.name.clone()))
                    .bind((format!("vis_{i}"), vis_to_string(&t.visibility)))
                    .bind((
                        format!("gen_{i}"),
                        serde_json::to_string(&t.generics).unwrap(),
                    ))
                    .bind((format!("fp_{i}"), t.file_path.clone()))
                    .bind((format!("ls_{i}"), t.line_start as i64))
                    .bind((format!("le_{i}"), t.line_end as i64))
                    .bind((format!("doc_{i}"), t.docstring.clone()))
                    .bind((format!("ext_{i}"), t.is_external))
                    .bind((format!("src_{i}"), t.source.clone()));
            }
            q.await.context("Failed to batch upsert traits")?;
        }
        Ok(())
    }

    /// Batch upsert enums.
    pub async fn batch_upsert_enums(&self, enums: &[EnumNode]) -> Result<()> {
        for chunk in enums.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPSERT type::record('enum', $key_{i}) SET \
                     name = $name_{i}, visibility = $vis_{i}, variants = $var_{i}, \
                     file_path = $fp_{i}, line_start = $ls_{i}, line_end = $le_{i}, \
                     docstring = $doc_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, e) in chunk.iter().enumerate() {
                let key = symbol_key(&e.file_path, &e.name, e.line_start);
                q = q
                    .bind((format!("key_{i}"), key))
                    .bind((format!("name_{i}"), e.name.clone()))
                    .bind((format!("vis_{i}"), vis_to_string(&e.visibility)))
                    .bind((
                        format!("var_{i}"),
                        serde_json::to_string(&e.variants).unwrap(),
                    ))
                    .bind((format!("fp_{i}"), e.file_path.clone()))
                    .bind((format!("ls_{i}"), e.line_start as i64))
                    .bind((format!("le_{i}"), e.line_end as i64))
                    .bind((format!("doc_{i}"), e.docstring.clone()));
            }
            q.await.context("Failed to batch upsert enums")?;
        }
        Ok(())
    }

    /// Batch upsert impl blocks.
    pub async fn batch_upsert_impls(&self, impls: &[ImplNode]) -> Result<()> {
        for chunk in impls.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPSERT type::record('impl', $key_{i}) SET \
                     for_type = $ft_{i}, trait_name = $tn_{i}, generics = $gen_{i}, \
                     where_clause = $wc_{i}, file_path = $fp_{i}, \
                     line_start = $ls_{i}, line_end = $le_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, imp) in chunk.iter().enumerate() {
                let key = impl_key(&imp.file_path, &imp.for_type, imp.line_start);
                q = q
                    .bind((format!("key_{i}"), key))
                    .bind((format!("ft_{i}"), imp.for_type.clone()))
                    .bind((format!("tn_{i}"), imp.trait_name.clone()))
                    .bind((
                        format!("gen_{i}"),
                        serde_json::to_string(&imp.generics).unwrap(),
                    ))
                    .bind((format!("wc_{i}"), imp.where_clause.clone()))
                    .bind((format!("fp_{i}"), imp.file_path.clone()))
                    .bind((format!("ls_{i}"), imp.line_start as i64))
                    .bind((format!("le_{i}"), imp.line_end as i64));
            }
            q.await.context("Failed to batch upsert impls")?;
        }
        Ok(())
    }

    /// Batch upsert imports.
    pub async fn batch_upsert_imports(&self, imports: &[ImportNode]) -> Result<()> {
        for chunk in imports.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "UPSERT type::record('import', $key_{i}) SET \
                     path = $path_{i}, alias = $alias_{i}, items = $items_{i}, \
                     file_path = $fp_{i}, line = $line_{i} \
                     RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, imp) in chunk.iter().enumerate() {
                let key = import_key(&imp.file_path, &imp.path, imp.line);
                q = q
                    .bind((format!("key_{i}"), key))
                    .bind((format!("path_{i}"), imp.path.clone()))
                    .bind((format!("alias_{i}"), imp.alias.clone()))
                    .bind((
                        format!("items_{i}"),
                        serde_json::to_string(&imp.items).unwrap(),
                    ))
                    .bind((format!("fp_{i}"), imp.file_path.clone()))
                    .bind((format!("line_{i}"), imp.line as i64));
            }
            q.await.context("Failed to batch upsert imports")?;
        }
        Ok(())
    }

    // ========================================================================
    // Relationship creation
    // ========================================================================

    /// Create an IMPORTS relationship between two files.
    pub async fn create_import_relationship(
        &self,
        from_file: &str,
        to_file: &str,
        import_path: &str,
    ) -> Result<()> {
        let from_rid = RecordId::new("file", from_file);
        let to_rid = RecordId::new("file", to_file);
        self.db
            .query("RELATE $from->`imports`->$to SET import_path = $ip RETURN NONE")
            .bind(("from", from_rid))
            .bind(("to", to_rid))
            .bind(("ip", import_path.to_string()))
            .await
            .context("Failed to create import relationship")?;
        Ok(())
    }

    /// Batch create IMPORTS relationships between files.
    pub async fn batch_create_import_relationships(
        &self,
        relationships: &[(String, String, String)],
    ) -> Result<()> {
        if relationships.is_empty() {
            return Ok(());
        }
        for chunk in relationships.chunks(50) {
            let mut query = String::from("BEGIN TRANSACTION;\n");
            for (i, _) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "RELATE $from_{i}->`imports`->$to_{i} \
                     SET import_path = $ip_{i} RETURN NONE;\n"
                ));
            }
            query.push_str("COMMIT TRANSACTION;");

            let mut q = self.db.query(&query);
            for (i, (from, to, ip)) in chunk.iter().enumerate() {
                q = q
                    .bind((format!("from_{i}"), RecordId::new("file", from.as_str())))
                    .bind((format!("to_{i}"), RecordId::new("file", to.as_str())))
                    .bind((format!("ip_{i}"), ip.clone()));
            }
            q.await
                .context("Failed to batch create import relationships")?;
        }
        Ok(())
    }

    /// Create a CALLS relationship between two functions.
    pub async fn create_call_relationship(
        &self,
        caller_id: &str,
        callee_name: &str,
        _project_id: Option<Uuid>,
        confidence: f64,
        reason: &str,
    ) -> Result<()> {
        // Find callee by name and create the relationship
        self.db
            .query(
                "LET $callee = (SELECT VALUE id FROM `function` WHERE name = $callee_name LIMIT 1);\
                 IF array::len($callee) > 0 THEN \
                     RELATE type::record('function', $caller_key)->`calls`->$callee[0] \
                     SET confidence = $conf, reason = $reason RETURN NONE \
                 END",
            )
            .bind(("caller_key", caller_id.to_string()))
            .bind(("callee_name", callee_name.to_string()))
            .bind(("conf", confidence))
            .bind(("reason", reason.to_string()))
            .await
            .context("Failed to create call relationship")?;
        Ok(())
    }

    /// Create a USES_TYPE relationship from a function to a type.
    pub async fn create_uses_type_relationship(
        &self,
        function_id: &str,
        type_name: &str,
    ) -> Result<()> {
        self.db
            .query(
                "LET $type_id = (SELECT VALUE id FROM `struct` WHERE name = $tn LIMIT 1);\
                 IF array::len($type_id) > 0 THEN \
                     RELATE type::record('function', $fkey)->uses_type->$type_id[0] RETURN NONE \
                 END",
            )
            .bind(("fkey", function_id.to_string()))
            .bind(("tn", type_name.to_string()))
            .await
            .context("Failed to create uses_type relationship")?;
        Ok(())
    }

    // ========================================================================
    // Trait & Type introspection
    // ========================================================================

    /// Find a trait by name.
    pub async fn find_trait_by_name(&self, name: &str) -> Result<Option<String>> {
        let mut response = self
            .db
            .query("SELECT name FROM `trait` WHERE name = $name LIMIT 1")
            .bind(("name", name.to_string()))
            .await
            .context("Failed to find trait")?;

        let results: Vec<NameRecord> = response.take(0)?;
        Ok(results.into_iter().next().map(|r| r.name))
    }

    /// Find types that implement a specific trait (via impl blocks).
    pub async fn find_trait_implementors(&self, trait_name: &str) -> Result<Vec<String>> {
        let mut response = self
            .db
            .query("SELECT for_type FROM `impl` WHERE trait_name = $tn GROUP BY for_type")
            .bind(("tn", trait_name.to_string()))
            .await
            .context("Failed to find trait implementors")?;

        let results: Vec<ForTypeRecord> = response.take(0)?;
        Ok(results.into_iter().map(|r| r.for_type).collect())
    }

    /// Get all traits implemented by a type (via impl blocks).
    pub async fn get_type_traits(&self, type_name: &str) -> Result<Vec<String>> {
        let mut response = self
            .db
            .query(
                "SELECT trait_name FROM `impl` \
                 WHERE for_type = $tn AND trait_name IS NOT NONE \
                 GROUP BY trait_name",
            )
            .bind(("tn", type_name.to_string()))
            .await
            .context("Failed to get type traits")?;

        let results: Vec<TraitNameFieldRecord> = response.take(0)?;
        Ok(results.into_iter().filter_map(|r| r.trait_name).collect())
    }

    /// Get trait info (is_external, source).
    pub async fn get_trait_info(&self, trait_name: &str) -> Result<Option<TraitInfoNode>> {
        let mut response = self
            .db
            .query("SELECT is_external, source FROM `trait` WHERE name = $name LIMIT 1")
            .bind(("name", trait_name.to_string()))
            .await
            .context("Failed to get trait info")?;

        let results: Vec<TraitInfoRecord> = response.take(0)?;
        Ok(results.into_iter().next().map(|r| TraitInfoNode {
            is_external: r.is_external,
            source: r.source,
        }))
    }

    /// Get trait implementors with file locations.
    pub async fn get_trait_implementors_detailed(
        &self,
        trait_name: &str,
    ) -> Result<Vec<TraitImplementorNode>> {
        let mut response = self
            .db
            .query(
                "SELECT for_type, file_path, line_start, line_end, trait_name \
                 FROM `impl` WHERE trait_name = $tn",
            )
            .bind(("tn", trait_name.to_string()))
            .await
            .context("Failed to get trait implementors detailed")?;

        let results: Vec<ImplDetailRecord> = response.take(0)?;
        Ok(results
            .into_iter()
            .map(|r| TraitImplementorNode {
                type_name: r.for_type,
                file_path: r.file_path,
                line: r.line_start as u32,
            })
            .collect())
    }

    /// Get all traits implemented by a type with details.
    pub async fn get_type_trait_implementations(
        &self,
        type_name: &str,
    ) -> Result<Vec<TypeTraitInfoNode>> {
        let mut response = self
            .db
            .query(
                "SELECT for_type, file_path, line_start, line_end, trait_name \
                 FROM `impl` WHERE for_type = $tn AND trait_name IS NOT NONE",
            )
            .bind(("tn", type_name.to_string()))
            .await
            .context("Failed to get type trait implementations")?;

        let impls: Vec<ImplDetailRecord> = response.take(0)?;

        let mut result = Vec::new();
        for imp in impls {
            if let Some(tn) = &imp.trait_name {
                let trait_info = self.get_trait_info(tn).await.ok().flatten();
                result.push(TypeTraitInfoNode {
                    name: tn.clone(),
                    full_path: None,
                    file_path: imp.file_path,
                    is_external: trait_info.as_ref().is_some_and(|t| t.is_external),
                    source: trait_info.and_then(|t| t.source),
                });
            }
        }
        Ok(result)
    }

    /// Get all impl blocks for a type with methods.
    pub async fn get_type_impl_blocks_detailed(
        &self,
        type_name: &str,
    ) -> Result<Vec<ImplBlockDetailNode>> {
        let mut response = self
            .db
            .query(
                "SELECT for_type, file_path, line_start, line_end, trait_name \
                 FROM `impl` WHERE for_type = $tn",
            )
            .bind(("tn", type_name.to_string()))
            .await
            .context("Failed to get type impl blocks")?;

        let impls: Vec<ImplDetailRecord> = response.take(0)?;

        let mut results = Vec::new();
        for imp in impls {
            let mut func_resp = self
                .db
                .query(
                    "SELECT name FROM `function` WHERE file_path = $fp AND \
                     line_start >= $ls AND line_end <= $le ORDER BY line_start",
                )
                .bind(("fp", imp.file_path.clone()))
                .bind(("ls", imp.line_start))
                .bind(("le", imp.line_end))
                .await
                .context("Failed to get impl methods")?;

            let methods: Vec<NameRecord> = func_resp.take(0)?;

            results.push(ImplBlockDetailNode {
                file_path: imp.file_path,
                line_start: imp.line_start as u32,
                line_end: imp.line_end as u32,
                trait_name: imp.trait_name,
                methods: methods.into_iter().map(|m| m.name).collect(),
            });
        }
        Ok(results)
    }

    // ========================================================================
    // File symbol summaries
    // ========================================================================

    /// Get function summaries for a file.
    pub async fn get_file_functions_summary(&self, path: &str) -> Result<Vec<FunctionSummaryNode>> {
        let mut response = self
            .db
            .query(
                "SELECT name, line_start, is_async, visibility, complexity, \
                 docstring, return_type, parameters \
                 FROM `function` WHERE file_path = $fp ORDER BY line_start",
            )
            .bind(("fp", path.to_string()))
            .await
            .context("Failed to get file functions summary")?;

        let results: Vec<FuncSummaryRecord> = response.take(0)?;
        Ok(results
            .into_iter()
            .map(|r| {
                let params: Vec<Parameter> = r
                    .parameters
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();
                let ret = r.return_type.as_deref().unwrap_or("()");
                let param_str = params
                    .iter()
                    .map(|p| {
                        if let Some(t) = &p.type_name {
                            format!("{}: {}", p.name, t)
                        } else {
                            p.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let sig = format!("fn {}({}) -> {}", r.name, param_str, ret);

                FunctionSummaryNode {
                    name: r.name,
                    signature: sig,
                    line: r.line_start as u32,
                    is_async: r.is_async,
                    is_public: r.visibility == "public",
                    complexity: r.complexity.unwrap_or(1) as u32,
                    docstring: r.docstring,
                }
            })
            .collect())
    }

    /// Get struct summaries for a file.
    pub async fn get_file_structs_summary(&self, path: &str) -> Result<Vec<StructSummaryNode>> {
        let mut response = self
            .db
            .query(
                "SELECT name, line_start, visibility, docstring \
                 FROM `struct` WHERE file_path = $fp ORDER BY line_start",
            )
            .bind(("fp", path.to_string()))
            .await
            .context("Failed to get file structs summary")?;

        let results: Vec<StructSummaryRecord> = response.take(0)?;
        Ok(results
            .into_iter()
            .map(|r| StructSummaryNode {
                name: r.name,
                line: r.line_start as u32,
                is_public: r.visibility == "public",
                docstring: r.docstring,
            })
            .collect())
    }

    // ========================================================================
    // Cleanup operations
    // ========================================================================

    /// Clean up all sync-generated data.
    pub async fn cleanup_sync_data(&self) -> Result<i64> {
        let mut total = 0i64;
        let tables = [
            "function", "struct", "trait", "enum", "impl", "import", "file",
        ];
        for table in &tables {
            let query = format!("SELECT count() AS count FROM `{}` GROUP ALL", table);
            let mut resp = self
                .db
                .query(&query)
                .await
                .context("Failed to count for cleanup")?;

            let counts: Vec<CountRecord> = resp.take(0)?;
            total += counts.into_iter().next().map(|c| c.count).unwrap_or(0);
        }

        self.db
            .query(
                "DELETE `function` RETURN NONE;\
                 DELETE `struct` RETURN NONE;\
                 DELETE `trait` RETURN NONE;\
                 DELETE `enum` RETURN NONE;\
                 DELETE `impl` RETURN NONE;\
                 DELETE `import` RETURN NONE;\
                 DELETE `file` RETURN NONE;\
                 DELETE `imports` RETURN NONE;\
                 DELETE `calls` RETURN NONE;\
                 DELETE contains RETURN NONE",
            )
            .await
            .context("Failed to cleanup sync data")?;

        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_core::test_helpers::*;

    async fn setup() -> IndentiaGraphStore {
        IndentiaGraphStore::new_memory().await.unwrap()
    }

    // ========================================================================
    // Function tests
    // ========================================================================

    #[tokio::test]
    async fn test_upsert_and_query_function() {
        let store = setup().await;
        let func = test_function("process_data", "/src/main.rs");
        store.upsert_function(&func).await.unwrap();

        let names = store.get_file_symbol_names("/src/main.rs").await.unwrap();
        assert!(
            names.functions.contains(&"process_data".to_string()),
            "Expected process_data in {:?}",
            names.functions
        );
    }

    #[tokio::test]
    async fn test_upsert_function_with_all_fields() {
        let store = setup().await;
        let func = FunctionNode {
            name: "complex_fn".to_string(),
            visibility: Visibility::Public,
            params: vec![
                Parameter {
                    name: "id".to_string(),
                    type_name: Some("u64".to_string()),
                },
                Parameter {
                    name: "name".to_string(),
                    type_name: Some("String".to_string()),
                },
            ],
            return_type: Some("Result<User>".to_string()),
            generics: vec!["T".to_string()],
            is_async: true,
            is_unsafe: false,
            complexity: 5,
            file_path: "/src/api.rs".to_string(),
            line_start: 10,
            line_end: 50,
            docstring: Some("Processes complex data".to_string()),
        };
        store.upsert_function(&func).await.unwrap();

        let summaries = store
            .get_file_functions_summary("/src/api.rs")
            .await
            .unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "complex_fn");
        assert!(summaries[0].is_async);
        assert!(summaries[0].is_public);
        assert_eq!(summaries[0].complexity, 5);
    }

    #[tokio::test]
    async fn test_batch_upsert_functions() {
        let store = setup().await;
        let funcs: Vec<FunctionNode> = (0..25)
            .map(|i| {
                let mut f = test_function(&format!("fn_{}", i), "/src/batch.rs");
                f.line_start = i as u32 * 10;
                f.line_end = i as u32 * 10 + 9;
                f
            })
            .collect();

        store.batch_upsert_functions(&funcs).await.unwrap();

        let names = store.get_file_symbol_names("/src/batch.rs").await.unwrap();
        assert_eq!(names.functions.len(), 25);
    }

    #[tokio::test]
    async fn test_function_summary() {
        let store = setup().await;
        let mut func = test_function("handler", "/src/routes.rs");
        func.is_async = true;
        func.complexity = 3;
        func.docstring = Some("Handle request".to_string());
        store.upsert_function(&func).await.unwrap();

        let summaries = store
            .get_file_functions_summary("/src/routes.rs")
            .await
            .unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "handler");
        assert!(summaries[0].is_async);
        assert_eq!(summaries[0].complexity, 3);
    }

    // ========================================================================
    // Struct tests
    // ========================================================================

    #[tokio::test]
    async fn test_upsert_and_query_struct() {
        let store = setup().await;
        let s = test_struct("User", "/src/models.rs");
        store.upsert_struct(&s).await.unwrap();

        let names = store.get_file_symbol_names("/src/models.rs").await.unwrap();
        assert!(names.structs.contains(&"User".to_string()));
    }

    #[tokio::test]
    async fn test_struct_with_inheritance() {
        let store = setup().await;
        let s = StructNode {
            name: "AdminUser".to_string(),
            visibility: Visibility::Public,
            generics: vec![],
            file_path: "/src/models.rs".to_string(),
            line_start: 20,
            line_end: 30,
            docstring: None,
            parent_class: Some("User".to_string()),
            interfaces: vec!["Serializable".to_string(), "Cloneable".to_string()],
        };
        store.upsert_struct(&s).await.unwrap();

        let summaries = store
            .get_file_structs_summary("/src/models.rs")
            .await
            .unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "AdminUser");
    }

    #[tokio::test]
    async fn test_batch_upsert_structs() {
        let store = setup().await;
        let structs: Vec<StructNode> = (0..15)
            .map(|i| {
                let mut s = test_struct(&format!("Struct_{}", i), "/src/types.rs");
                s.line_start = i as u32 * 10;
                s.line_end = i as u32 * 10 + 9;
                s
            })
            .collect();

        store.batch_upsert_structs(&structs).await.unwrap();

        let names = store.get_file_symbol_names("/src/types.rs").await.unwrap();
        assert_eq!(names.structs.len(), 15);
    }

    #[tokio::test]
    async fn test_struct_summary() {
        let store = setup().await;
        let mut s = test_struct("Config", "/src/config.rs");
        s.docstring = Some("Application config".to_string());
        store.upsert_struct(&s).await.unwrap();

        let summaries = store
            .get_file_structs_summary("/src/config.rs")
            .await
            .unwrap();
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0].is_public);
        assert_eq!(
            summaries[0].docstring,
            Some("Application config".to_string())
        );
    }

    // ========================================================================
    // Trait tests
    // ========================================================================

    #[tokio::test]
    async fn test_upsert_and_find_trait() {
        let store = setup().await;
        let t = test_trait("GraphStore", "/src/traits.rs");
        store.upsert_trait(&t).await.unwrap();

        let found = store.find_trait_by_name("GraphStore").await.unwrap();
        assert_eq!(found, Some("GraphStore".to_string()));
    }

    #[tokio::test]
    async fn test_find_trait_not_found() {
        let store = setup().await;
        let found = store.find_trait_by_name("Nonexistent").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_trait_info() {
        let store = setup().await;
        let mut t = test_trait("Serialize", "/src/external.rs");
        t.is_external = true;
        t.source = Some("serde".to_string());
        store.upsert_trait(&t).await.unwrap();

        let info = store.get_trait_info("Serialize").await.unwrap();
        assert!(info.is_some());
        let info = info.unwrap();
        assert!(info.is_external);
        assert_eq!(info.source, Some("serde".to_string()));
    }

    #[tokio::test]
    async fn test_batch_upsert_traits() {
        let store = setup().await;
        let traits: Vec<TraitNode> = (0..10)
            .map(|i| {
                let mut t = test_trait(&format!("Trait_{}", i), "/src/traits.rs");
                t.line_start = i as u32 * 10;
                t.line_end = i as u32 * 10 + 9;
                t
            })
            .collect();

        store.batch_upsert_traits(&traits).await.unwrap();

        let names = store.get_file_symbol_names("/src/traits.rs").await.unwrap();
        assert_eq!(names.traits.len(), 10);
    }

    // ========================================================================
    // Enum tests
    // ========================================================================

    #[tokio::test]
    async fn test_upsert_and_query_enum() {
        let store = setup().await;
        let e = test_enum("Status", "/src/types.rs");
        store.upsert_enum(&e).await.unwrap();

        let names = store.get_file_symbol_names("/src/types.rs").await.unwrap();
        assert!(names.enums.contains(&"Status".to_string()));
    }

    #[tokio::test]
    async fn test_batch_upsert_enums() {
        let store = setup().await;
        let enums: Vec<EnumNode> = (0..8)
            .map(|i| {
                let mut e = test_enum(&format!("Enum_{}", i), "/src/enums.rs");
                e.line_start = i as u32 * 10;
                e.line_end = i as u32 * 10 + 9;
                e
            })
            .collect();

        store.batch_upsert_enums(&enums).await.unwrap();

        let names = store.get_file_symbol_names("/src/enums.rs").await.unwrap();
        assert_eq!(names.enums.len(), 8);
    }

    // ========================================================================
    // Impl tests
    // ========================================================================

    #[tokio::test]
    async fn test_upsert_impl() {
        let store = setup().await;
        let imp = test_impl("User", "/src/models.rs");
        store.upsert_impl(&imp).await.unwrap();
    }

    #[tokio::test]
    async fn test_upsert_impl_with_trait() {
        let store = setup().await;
        let t = test_trait("Display", "/src/std.rs");
        store.upsert_trait(&t).await.unwrap();

        let mut imp = test_impl("User", "/src/models.rs");
        imp.trait_name = Some("Display".to_string());
        store.upsert_impl(&imp).await.unwrap();

        let impls = store.find_trait_implementors("Display").await.unwrap();
        assert!(impls.contains(&"User".to_string()));
    }

    #[tokio::test]
    async fn test_batch_upsert_impls() {
        let store = setup().await;
        let impls: Vec<ImplNode> = (0..10)
            .map(|i| {
                let mut imp = test_impl(&format!("Type_{}", i), "/src/impls.rs");
                imp.line_start = i as u32 * 20;
                imp.line_end = i as u32 * 20 + 19;
                imp
            })
            .collect();

        store.batch_upsert_impls(&impls).await.unwrap();
    }

    // ========================================================================
    // Import tests
    // ========================================================================

    #[tokio::test]
    async fn test_upsert_import() {
        let store = setup().await;
        let imp = test_import("crate::models::User", "/src/main.rs");
        store.upsert_import(&imp).await.unwrap();

        let paths = store
            .get_file_import_paths_list("/src/main.rs")
            .await
            .unwrap();
        assert!(paths.contains(&"crate::models::User".to_string()));
    }

    #[tokio::test]
    async fn test_batch_upsert_imports() {
        let store = setup().await;
        let imports: Vec<ImportNode> = (0..12)
            .map(|i| {
                let mut imp = test_import(&format!("crate::module_{}", i), "/src/lib.rs");
                imp.line = i as u32 + 1;
                imp
            })
            .collect();

        store.batch_upsert_imports(&imports).await.unwrap();

        let paths = store
            .get_file_import_paths_list("/src/lib.rs")
            .await
            .unwrap();
        assert_eq!(paths.len(), 12);
    }

    // ========================================================================
    // Relationship tests
    // ========================================================================

    #[tokio::test]
    async fn test_create_import_relationship() {
        let store = setup().await;
        let f1 = test_file("/src/main.rs");
        let f2 = test_file("/src/lib.rs");
        store.upsert_file(&f1).await.unwrap();
        store.upsert_file(&f2).await.unwrap();

        store
            .create_import_relationship("/src/main.rs", "/src/lib.rs", "crate::lib")
            .await
            .unwrap();

        let imports = store.get_file_direct_imports("/src/main.rs").await.unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "/src/lib.rs");
    }

    #[tokio::test]
    async fn test_batch_create_import_relationships() {
        let store = setup().await;
        let files = vec![
            test_file("/src/a.rs"),
            test_file("/src/b.rs"),
            test_file("/src/c.rs"),
        ];
        for f in &files {
            store.upsert_file(f).await.unwrap();
        }

        let rels = vec![
            (
                "/src/a.rs".to_string(),
                "/src/b.rs".to_string(),
                "crate::b".to_string(),
            ),
            (
                "/src/a.rs".to_string(),
                "/src/c.rs".to_string(),
                "crate::c".to_string(),
            ),
        ];
        store
            .batch_create_import_relationships(&rels)
            .await
            .unwrap();

        let imports = store.get_file_direct_imports("/src/a.rs").await.unwrap();
        assert_eq!(imports.len(), 2);
    }

    // ========================================================================
    // Trait implementor tests
    // ========================================================================

    #[tokio::test]
    async fn test_find_trait_implementors() {
        let store = setup().await;
        let t = test_trait("Handler", "/src/traits.rs");
        store.upsert_trait(&t).await.unwrap();

        let mut imp1 = test_impl("UserHandler", "/src/handlers.rs");
        imp1.trait_name = Some("Handler".to_string());
        imp1.line_start = 10;
        let mut imp2 = test_impl("AdminHandler", "/src/handlers.rs");
        imp2.trait_name = Some("Handler".to_string());
        imp2.line_start = 50;

        store.upsert_impl(&imp1).await.unwrap();
        store.upsert_impl(&imp2).await.unwrap();

        let impls = store.find_trait_implementors("Handler").await.unwrap();
        assert_eq!(impls.len(), 2);
        assert!(impls.contains(&"UserHandler".to_string()));
        assert!(impls.contains(&"AdminHandler".to_string()));
    }

    #[tokio::test]
    async fn test_get_type_traits() {
        let store = setup().await;

        let mut imp1 = test_impl("User", "/src/models.rs");
        imp1.trait_name = Some("Display".to_string());
        imp1.line_start = 10;
        let mut imp2 = test_impl("User", "/src/models.rs");
        imp2.trait_name = Some("Debug".to_string());
        imp2.line_start = 20;

        store.upsert_impl(&imp1).await.unwrap();
        store.upsert_impl(&imp2).await.unwrap();

        let traits = store.get_type_traits("User").await.unwrap();
        assert_eq!(traits.len(), 2);
        assert!(traits.contains(&"Display".to_string()));
        assert!(traits.contains(&"Debug".to_string()));
    }

    #[tokio::test]
    async fn test_get_trait_implementors_detailed() {
        let store = setup().await;
        let t = test_trait("Serialize", "/src/serde.rs");
        store.upsert_trait(&t).await.unwrap();

        let mut imp = test_impl("Config", "/src/config.rs");
        imp.trait_name = Some("Serialize".to_string());
        imp.line_start = 15;
        store.upsert_impl(&imp).await.unwrap();

        let detailed = store
            .get_trait_implementors_detailed("Serialize")
            .await
            .unwrap();
        assert_eq!(detailed.len(), 1);
        assert_eq!(detailed[0].type_name, "Config");
        assert_eq!(detailed[0].file_path, "/src/config.rs");
        assert_eq!(detailed[0].line, 15);
    }

    #[tokio::test]
    async fn test_get_type_trait_implementations() {
        let store = setup().await;

        let mut t = test_trait("Clone", "/src/std.rs");
        t.is_external = true;
        t.source = Some("std".to_string());
        store.upsert_trait(&t).await.unwrap();

        let mut imp = test_impl("MyStruct", "/src/lib.rs");
        imp.trait_name = Some("Clone".to_string());
        store.upsert_impl(&imp).await.unwrap();

        let impls = store
            .get_type_trait_implementations("MyStruct")
            .await
            .unwrap();
        assert_eq!(impls.len(), 1);
        assert_eq!(impls[0].name, "Clone");
        assert!(impls[0].is_external);
        assert_eq!(impls[0].source, Some("std".to_string()));
    }

    // ========================================================================
    // Cleanup tests
    // ========================================================================

    #[tokio::test]
    async fn test_cleanup_sync_data() {
        let store = setup().await;

        let f = test_file("/src/clean.rs");
        store.upsert_file(&f).await.unwrap();
        let func = test_function("cleanup_fn", "/src/clean.rs");
        store.upsert_function(&func).await.unwrap();
        let s = test_struct("CleanStruct", "/src/clean.rs");
        store.upsert_struct(&s).await.unwrap();

        let deleted = store.cleanup_sync_data().await.unwrap();
        assert!(
            deleted >= 3,
            "Should have deleted at least 3 items, got {}",
            deleted
        );

        assert!(store.get_file("/src/clean.rs").await.unwrap().is_none());
        let names = store.get_file_symbol_names("/src/clean.rs").await.unwrap();
        assert!(names.functions.is_empty());
        assert!(names.structs.is_empty());
    }

    // ========================================================================
    // Cross-module integration tests
    // ========================================================================

    #[tokio::test]
    async fn test_file_with_symbols_full_lifecycle() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let file = test_file_for_project("/src/lifecycle.rs", project.id);
        store.upsert_file(&file).await.unwrap();

        let func = test_function("handle", "/src/lifecycle.rs");
        store.upsert_function(&func).await.unwrap();
        let s = test_struct("Request", "/src/lifecycle.rs");
        store.upsert_struct(&s).await.unwrap();
        let t = test_trait("Handler", "/src/lifecycle.rs");
        store.upsert_trait(&t).await.unwrap();
        let e = test_enum("Method", "/src/lifecycle.rs");
        store.upsert_enum(&e).await.unwrap();

        let names = store
            .get_file_symbol_names("/src/lifecycle.rs")
            .await
            .unwrap();
        assert_eq!(names.functions, vec!["handle"]);
        assert_eq!(names.structs, vec!["Request"]);
        assert_eq!(names.traits, vec!["Handler"]);
        assert_eq!(names.enums, vec!["Method"]);

        store.delete_file("/src/lifecycle.rs").await.unwrap();
        let names = store
            .get_file_symbol_names("/src/lifecycle.rs")
            .await
            .unwrap();
        assert!(names.functions.is_empty());
        assert!(names.structs.is_empty());
        assert!(names.traits.is_empty());
        assert!(names.enums.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_files_with_imports() {
        let store = setup().await;
        let project = test_project();
        store.create_project(&project).await.unwrap();

        let f_models = test_file_for_project("/src/models.rs", project.id);
        store.upsert_file(&f_models).await.unwrap();
        store
            .upsert_struct(&test_struct("User", "/src/models.rs"))
            .await
            .unwrap();

        let f_handlers = test_file_for_project("/src/handlers.rs", project.id);
        store.upsert_file(&f_handlers).await.unwrap();
        store
            .upsert_function(&test_function("get_user", "/src/handlers.rs"))
            .await
            .unwrap();

        store
            .create_import_relationship("/src/handlers.rs", "/src/models.rs", "crate::models")
            .await
            .unwrap();

        let imports = store
            .get_file_direct_imports("/src/handlers.rs")
            .await
            .unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].path, "/src/models.rs");
        assert_eq!(imports[0].language, "rust");
    }

    #[tokio::test]
    async fn test_visibility_roundtrip() {
        let visibilities = vec![
            (Visibility::Public, "public"),
            (Visibility::Private, "private"),
            (Visibility::Crate, "crate"),
            (Visibility::Super, "super"),
        ];

        for (vis, expected_str) in visibilities {
            assert_eq!(vis_to_string(&vis), expected_str);
            assert_eq!(string_to_vis(expected_str), vis);
        }
    }
}
