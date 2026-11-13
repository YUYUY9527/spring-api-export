// Markdown exporter: human-readable API doc with Java comments inline.

use crate::model::{Endpoint, Schema};
use crate::openapi::{openapi_type, SchemaSet};
use std::collections::BTreeMap;

pub fn render(endpoints: &[Endpoint], refs: &SchemaSet) -> String {
    let mut out = String::new();
    out.push_str("# 接口文档\n\n");
    out.push_str(&format!(
        "> 由 OpenIDEA 插件 Spring Api Export 自动生成，共 {} 个接口\n\n",
        endpoints.len()
    ));

    // group by controller, keep file order
    let mut order: Vec<String> = Vec::new();
    let mut grouped: BTreeMap<String, Vec<&Endpoint>> = BTreeMap::new();
    for e in endpoints {
        if !grouped.contains_key(&e.controller) {
            order.push(e.controller.clone());
        }
        grouped.entry(e.controller.clone()).or_default().push(e);
    }

    for ctrl in &order {
        out.push_str(&format!("## {}\n\n", ctrl));
        let cd = grouped[ctrl][0].controller_desc.clone();
        if !cd.is_empty() {
            out.push_str(&format!("{cd}\n\n"));
        }
        for ep in &grouped[ctrl] {
            out.push_str(&format!("### {} {}\n\n", ep.method, ep.path));
            if !ep.summary.is_empty() {
                out.push_str(&format!("**{}**\n\n", ep.summary));
            }
            if !ep.description.is_empty() {
                for line in ep.description.lines() {
                    out.push_str(&format!("{}\n", line));
                }
                out.push('\n');
            }
            out.push_str(&format!("- 处理方法: `{}#{}`\n", ctrl, ep.handler));

            let params: Vec<_> = ep.params.iter().filter(|p| p.location != "object").collect();
            if !params.is_empty() {
                out.push_str("\n**请求参数**\n\n");
                out.push_str("| 参数 | 位置 | 类型 | 必填 | 默认值 | 说明 |\n");
                out.push_str("| --- | --- | --- | --- | --- | --- |\n");
                for p in &params {
                    out.push_str(&format!(
                        "| `{}` | {} | `{}` | {} | {} | {} |\n",
                        p.name,
                        p.location,
                        p.java_type,
                        if p.required || p.location == "path" { "是" } else { "否" },
                        p.default.clone().unwrap_or_else(|| "-".into()),
                        p.desc
                    ));
                }
            }

            if let Some(body) = &ep.request_body {
                out.push_str("\n**请求体**");
                if !ep.request_desc.is_empty() {
                    out.push_str(&format!("（{}）", ep.request_desc));
                }
                out.push_str(&format!(": `{}`\n", body));
                if let Some(key) = inner_dto_name(body) {
                    if let Some(sc) = refs.schemas.get(&key) {
                        push_field_table(&mut out, sc, refs, 0);
                    }
                }
            }

            out.push_str("\n**响应**");
            match (&ep.response_envelope, &ep.response) {
                (Some(w), Some(p)) => out.push_str(&format!(": `{}<{}>`", w, p)),
                (Some(w), None) => out.push_str(&format!(": `{}`", w)),
                (None, Some(p)) => out.push_str(&format!(": `{}`", p)),
                (None, None) => out.push_str(": 无"),
            }
            if !ep.return_desc.is_empty() {
                out.push_str(&format!("（{}）", ep.return_desc));
            }
            out.push('\n');
            if let Some(w) = &ep.response_envelope {
                let mut sc = refs
                    .schemas
                    .get(w)
                    .cloned()
                    .or_else(|| crate::spring::envelope_shape(w));
                if let Some(s) = &mut sc {
                    // show the real payload type on the envelope's `data` field
                    if let Some(f) = s.fields.iter_mut().find(|f| f.name == "data") {
                        f.java_type = ep.response.clone().unwrap_or_else(|| "object".into());
                    }
                }
                if let Some(s) = sc {
                    push_field_table(&mut out, &s, refs, 0);
                }
            } else if let Some(p) = &ep.response {
                if let Some(key) = inner_dto_name(p) {
                    if let Some(sc) = refs.schemas.get(&key) {
                        push_field_table(&mut out, sc, refs, 0);
                    }
                }
            }
            out.push_str("\n---\n\n");
        }
    }
    out
}

fn push_field_table(out: &mut String, sc: &Schema, refs: &SchemaSet, depth: usize) {
    if sc.fields.is_empty() {
        return;
    }
    let pad = "  ".repeat(depth);
    out.push_str(&format!("\n{}**{} 字段**", pad, sc.name));
    if !sc.desc.is_empty() {
        out.push_str(&format!("（{}）", sc.desc));
    }
    out.push_str("\n\n");
    out.push_str(&format!("{}| 字段 | 类型 | 必填 | 说明 |\n", pad));
    out.push_str(&format!("{}| --- | --- | --- | --- |\n", pad));
    for f in &sc.fields {
        out.push_str(&format!(
            "{}| `{}` | `{}` | {} | {} |\n",
            pad,
            f.name,
            f.java_type,
            if f.required { "是" } else { "否" },
            f.desc
        ));
    }
    // one level of nested DTOs inline
    if depth < 1 {
        let mut seen = Vec::new();
        for f in &sc.fields {
            if let Some(key) = inner_dto_name(&f.java_type) {
                if let Some(nested) = refs.schemas.get(&key) {
                    if !seen.contains(&nested.name) {
                        seen.push(nested.name.clone());
                        push_field_table(out, nested, refs, depth + 1);
                    }
                }
            }
        }
    }
}

const BUILTIN: &[&str] = &[
    "String", "CharSequence", "int", "Integer", "long", "Long", "short", "Short",
    "double", "Double", "float", "Float", "boolean", "Boolean", "BigDecimal",
    "BigInteger", "Date", "LocalDate", "LocalDateTime", "LocalTime", "UUID",
    "Object", "void", "Void", "byte", "Byte", "char", "Character", "MultipartFile",
];

/// Extracts the DTO class name from a type expression, looking through
/// collections (`List<UserVo>`), arrays and paging wrappers (`IPage<UserVo>`).
fn inner_dto_name(t: &str) -> Option<String> {
    let mut cur = t.trim().to_string();
    for _ in 0..8 {
        if cur.ends_with('>') {
            if let Some(o) = cur.find('<') {
                let base = crate::spring::simple_type_name(&cur[..o]);
                let inner = cur[o + 1..cur.len() - 1].trim().to_string();
                if matches!(
                    base.as_str(),
                    "List" | "Set" | "Collection" | "ArrayList" | "LinkedList" | "IPage" | "Page"
                ) && !inner.is_empty()
                {
                    cur = inner;
                    continue;
                }
                return if BUILTIN.contains(&base.as_str()) { None } else { Some(base) };
            }
        }
        if let Some(b) = cur.strip_suffix("[]") {
            cur = b.trim().to_string();
            continue;
        }
        let n = crate::spring::simple_type_name(&cur);
        return if n.is_empty() || BUILTIN.contains(&n.as_str()) { None } else { Some(n) };
    }
    None
}

fn split_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '<' => { depth += 1; cur.push(c); }
            '>' => { depth -= 1; cur.push(c); }
            ',' if depth == 0 => { out.push(cur.trim().to_string()); cur.clear(); }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Collects every DTO name mentioned by a type expression (recursively).
fn collect_refs(t: &str, out: &mut Vec<String>) {
    let s = t.trim();
    if s.is_empty() {
        return;
    }
    if let Some(o) = s.find('<') {
        let base = crate::spring::simple_type_name(&s[..o]);
        let inner = s[o + 1..s.len().saturating_sub(1)].trim().to_string();
        let transparent = matches!(
            base.as_str(),
            "List" | "Set" | "Collection" | "ArrayList" | "LinkedList" | "Map" | "HashMap"
                | "IPage" | "Page"
        );
        if !transparent && !base.is_empty() && !BUILTIN.contains(&base.as_str()) {
            out.push(base);
        }
        for part in split_commas(&inner) {
            collect_refs(&part, out);
        }
        return;
    }
    if let Some(b) = s.strip_suffix("[]") {
        collect_refs(b, out);
        return;
    }
    let (ty, _) = openapi_type(s);
    if ty == "ref" {
        let n = crate::spring::simple_type_name(s);
        if !n.is_empty() {
            out.push(n);
        }
    }
}

/// Builds the SchemaSet needed for the given endpoints by resolving DTO classes.
pub struct DtoResolver {
    /// DTO class name -> parsed class
    pub lookup: BTreeMap<String, crate::java::JavaClass>,
}

impl DtoResolver {
    pub fn new() -> Self {
        DtoResolver { lookup: BTreeMap::new() }
    }

    pub fn add_class(&mut self, cls: crate::java::JavaClass) {
        self.lookup.entry(cls.name.clone()).or_insert(cls);
    }

    /// Resolves schemas for all DTOs referenced by `endpoints`, transitively.
    pub fn resolve(&self, endpoints: &[Endpoint]) -> SchemaSet {
        let mut schemas: BTreeMap<String, Schema> = BTreeMap::new();
        let mut wanted: Vec<String> = Vec::new();
        for e in endpoints {
            if let Some(b) = &e.request_body {
                collect_refs(b, &mut wanted);
            }
            if let Some(w) = &e.response_envelope {
                wanted.push(w.clone());
            }
            if let Some(r) = &e.response {
                collect_refs(r, &mut wanted);
            }
        }
        while let Some(name) = wanted.pop() {
            if schemas.contains_key(&name) {
                continue;
            }
            let Some(cls) = self.lookup.get(&name) else {
                continue;
            };
            let sc = crate::spring::schema_for_class(cls);
            for f in &sc.fields {
                collect_refs(&f.java_type, &mut wanted);
            }
            schemas.insert(name, sc);
        }
        SchemaSet { schemas }
    }
}
