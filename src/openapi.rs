// OpenAPI 3.0 JSON writer, backed by serde_json (correctness over bundle size).

use crate::model::{Endpoint, Schema};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub struct OpenApiWriter {
    pub title: String,
}

pub struct SchemaSet {
    /// DTO schemas to embed in components, keyed by name.
    pub schemas: BTreeMap<String, Schema>,
}

impl OpenApiWriter {
    pub fn render(&self, endpoints: &[Endpoint], refs: &SchemaSet) -> String {
        let mut paths: Map<String, Value> = Map::new();
        // group by path, preserving order
        let mut order: Vec<String> = Vec::new();
        let mut grouped: BTreeMap<String, Vec<&Endpoint>> = BTreeMap::new();
        for e in endpoints {
            if !grouped.contains_key(&e.path) {
                order.push(e.path.clone());
            }
            grouped.entry(e.path.clone()).or_default().push(e);
        }

        // `@RequestMapping(method = {GET, POST})` yields one endpoint per verb from the
        // same handler, so the bare `controller_handler` id would repeat. Only those
        // handlers get the verb appended; everything else keeps its existing id.
        let mut handler_counts: BTreeMap<(String, String), usize> = BTreeMap::new();
        for e in endpoints {
            *handler_counts
                .entry((e.controller.clone(), e.handler.clone()))
                .or_insert(0) += 1;
        }

        for path in &order {
            let mut ops = Map::new();
            for ep in &grouped[path] {
                let mut op = Map::new();
                op.insert("summary".into(), json!(ep.summary));
                if !ep.description.is_empty() {
                    op.insert("description".into(), json!(ep.description));
                }
                let mut op_id = format!("{}_{}", snake(&ep.controller), snake(&ep.handler));
                if handler_counts
                    .get(&(ep.controller.clone(), ep.handler.clone()))
                    .copied()
                    .unwrap_or(1)
                    > 1
                {
                    op_id = format!("{}_{}", op_id, ep.method.to_lowercase());
                }
                op.insert("operationId".into(), json!(op_id));
                op.insert("tags".into(), json!([ep.controller]));

                let params: Vec<Value> = ep
                    .params
                    .iter()
                    .filter(|p| p.location != "object")
                    .map(|p| {
                        let mut m = Map::new();
                        m.insert("name".into(), json!(p.name));
                        m.insert("in".into(), json!(p.location));
                        m.insert("required".into(), json!(p.required || p.location == "path"));
                        if !p.desc.is_empty() {
                            m.insert("description".into(), json!(p.desc));
                        }
                        let mut schema_val = schema_value(&p.java_type, refs);
                        if let Value::Object(o) = &mut schema_val {
                            if let Some(d) = &p.default {
                                o.insert("default".into(), json!(d));
                            }
                            if p.java_type == "object" {
                                o.entry("type").or_insert_with(|| json!("object"));
                            }
                        }
                        if p.java_type == "object" {
                            m.insert("style".into(), json!("deepObject"));
                            m.insert("explode".into(), json!(true));
                        }
                        m.insert("schema".into(), schema_val);
                        Value::Object(m)
                    })
                    .collect();
                if !params.is_empty() {
                    op.insert("parameters".into(), Value::Array(params));
                }

                if let Some(body) = &ep.request_body {
                    let mut rb = Map::new();
                    if !ep.request_desc.is_empty() {
                        rb.insert("description".into(), json!(ep.request_desc));
                    }
                    rb.insert("required".into(), json!(true));
                    rb.insert(
                        "content".into(),
                        json!({ "application/json": { "schema": schema_value(body, refs) } }),
                    );
                    op.insert("requestBody".into(), Value::Object(rb));
                }

                let mut resp = Map::new();
                resp.insert(
                    "description".into(),
                    json!(if ep.return_desc.is_empty() { "成功".to_string() } else { ep.return_desc.clone() }),
                );
                if let Some(sv) = response_schema(ep, refs) {
                    resp.insert("content".into(), json!({ "application/json": { "schema": sv } }));
                }
                op.insert("responses".into(), json!({ "200": Value::Object(resp) }));
                ops.insert(ep.method.to_lowercase(), Value::Object(op));
            }
            paths.insert(path.clone(), Value::Object(ops));
        }

        let mut doc = Map::new();
        doc.insert("openapi".into(), json!("3.0.3"));
        doc.insert(
            "info".into(),
            json!({
                "title": self.title,
                "version": "1.0.0",
                "description": "Generated by Spring Api Export plugin for OpenIDEA",
            }),
        );
        doc.insert("paths".into(), Value::Object(paths));

        if !refs.schemas.is_empty() {
            let mut comps = Map::new();
            for (name, sc) in &refs.schemas {
                comps.insert(name.clone(), object_schema(sc, refs));
            }
            doc.insert("components".into(), json!({ "schemas": Value::Object(comps) }));
        }
        serde_json::to_string_pretty(&Value::Object(doc)).unwrap_or_else(|_| "{}".into())
    }
}

/// Response schema: the result envelope (with `data` replaced by the real
/// payload) when the method wraps its return value, otherwise the payload.
fn response_schema(ep: &Endpoint, refs: &SchemaSet) -> Option<Value> {
    let payload = ep.response.as_deref().map(|t| schema_value(t, refs));
    match &ep.response_envelope {
        Some(w) => {
            let mut base = match refs.schemas.get(w) {
                Some(sc) => object_schema(sc, refs),
                None => match crate::spring::envelope_shape(w) {
                    Some(s) => object_schema(&s, refs),
                    None => json!({ "type": "object" }),
                },
            };
            if let Some(props) = base.get_mut("properties").and_then(|v| v.as_object_mut()) {
                props.insert("data".into(), payload.unwrap_or_else(|| json!({ "type": "object" })));
            }
            Some(base)
        }
        None => payload,
    }
}

/// `IPage<UserVo>` / `Page<UserVo>` -> `UserVo` (mybatis-plus paging wrappers).
fn ipage_inner(t: &str) -> Option<String> {
    let t = t.trim();
    if !t.ends_with('>') {
        return None;
    }
    let o = t.find('<')?;
    let base = crate::spring::simple_type_name(&t[..o]);
    if base == "IPage" || base == "Page" {
        Some(t[o + 1..t.len() - 1].trim().to_string())
    } else {
        None
    }
}

/// `{ "$ref": ... }` or primitive, recursing through arrays.
pub fn schema_value(type_name: &str, refs: &SchemaSet) -> Value {
    let t = type_name.trim();
    if let Some(inner) = array_inner(t) {
        return json!({ "type": "array", "items": schema_value(inner, refs) });
    }
    if let Some(inner) = ipage_inner(t) {
        return json!({
            "type": "object",
            "description": "分页结果（IPage 结构，插件按约定推断）",
            "properties": {
                "records": { "type": "array", "items": schema_value(&inner, refs) },
                "total": { "type": "integer", "format": "int64" },
                "size": { "type": "integer", "format": "int64" },
                "current": { "type": "integer", "format": "int64" },
                "pages": { "type": "integer", "format": "int64" }
            }
        });
    }
    let (ty, fmt) = openapi_type(t);
    if ty == "ref" {
        let name = crate::spring::simple_type_name(t);
        if refs.schemas.contains_key(&name) {
            return json!({ "$ref": format!("#/components/schemas/{}", name) });
        }
        // Source of this DTO is outside the project: keep the shape usable
        // instead of emitting a dangling $ref that many importers reject.
        return json!({ "type": "object", "description": format!("{}（未找到源码）", name) });
    }
    let mut m = Map::new();
    m.insert("type".into(), json!(ty));
    if let Some(f) = fmt {
        m.insert("format".into(), json!(f));
    }
    Value::Object(m)
}

/// Full object schema for a DTO, with per-field comments; nested DTOs become $refs.
pub fn object_schema(schema: &Schema, refs: &SchemaSet) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), json!("object"));
    if !schema.desc.is_empty() {
        m.insert("description".into(), json!(schema.desc));
    }
    let mut props = Map::new();
    let mut required: Vec<Value> = Vec::new();
    for f in &schema.fields {
        let mut fm = schema_value(&f.java_type, refs);
        if let Value::Object(o) = &mut fm {
            if !f.desc.is_empty() {
                o.insert("description".into(), json!(f.desc));
            }
        }
        props.insert(f.name.clone(), fm);
        if f.required {
            required.push(json!(f.name));
        }
    }
    m.insert("properties".into(), Value::Object(props));
    if !required.is_empty() {
        m.insert("required".into(), Value::Array(required));
    }
    Value::Object(m)
}

fn array_inner(t: &str) -> Option<&str> {
    for w in ["List", "Set", "Collection", "ArrayList", "LinkedList"] {
        let prefix = format!("{}<", w);
        if t.starts_with(&prefix) && t.ends_with('>') {
            return Some(t[prefix.len()..t.len() - 1].trim());
        }
    }
    if let Some(base) = t.strip_suffix("[]") {
        if base.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
            return Some(base);
        }
    }
    None
}

/// Java type -> (OpenAPI type, format). "ref" means a $ref to a DTO.
pub fn openapi_type(java_type: &str) -> (String, Option<String>) {
    let t = java_type.trim();
    let base = t.split('<').next().unwrap_or(t).trim_end_matches("[]");
    let base = base.rsplit('.').next().unwrap_or(base);
    match base {
        "int" | "Integer" => ("integer".into(), Some("int32".into())),
        "long" | "Long" | "BigInteger" => ("integer".into(), Some("int64".into())),
        "short" | "Short" => ("integer".into(), None),
        "double" | "Double" | "float" | "Float" | "BigDecimal" => ("number".into(), Some("double".into())),
        "boolean" | "Boolean" => ("boolean".into(), None),
        "char" | "Character" | "String" | "CharSequence" => ("string".into(), None),
        "Date" | "LocalDateTime" | "Instant" | "Timestamp" => ("string".into(), Some("date-time".into())),
        "LocalDate" => ("string".into(), Some("date".into())),
        "LocalTime" => ("string".into(), None),
        "UUID" => ("string".into(), Some("uuid".into())),
        "byte" | "Byte" => ("string".into(), Some("byte".into())),
        "Object" | "object" | "Map" | "HashMap" | "Void" | "void" => ("object".into(), None),
        "MultipartFile" => ("string".into(), Some("binary".into())),
        _ => ("ref".into(), None),
    }
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}
