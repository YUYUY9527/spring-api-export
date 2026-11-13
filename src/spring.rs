// Spring-specific mapping: JavaClass -> Endpoint / Schema.

use crate::java::{ann_path, is_keyword, JavaClass, MethodRaw};
use crate::model::{Endpoint, FieldDoc, Param, Schema};

/// True when the class looks like a Spring controller.
pub fn is_controller(cls: &JavaClass) -> bool {
    cls.anns.iter().any(|a| {
        matches!(
            a.name.as_str(),
            "RestController" | "Controller" | "RequestMapping" // RequestMapping alone is rare but valid
        )
    })
}

/// Base path from class-level @RequestMapping("/api/user").
pub fn base_path(cls: &JavaClass) -> String {
    for a in &cls.anns {
        if a.name == "RequestMapping" {
            if let Some(p) = ann_path(a) {
                return p;
            }
        }
    }
    String::new()
}

/// Extract the path variable names, in order, from a path template.
fn path_variables(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = path.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '}' {
                j += 1;
            }
            let inner: String = chars[i + 1..j.min(chars.len())].iter().collect();
            // `id` or `id:regex`
            let name = inner.split(':').next().unwrap_or("").trim().to_string();
            if !name.is_empty() {
                out.push(name);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn join_path(base: &str, sub: &str) -> String {
    let mut p = String::new();
    if !base.is_empty() {
        p.push_str(base.trim_start_matches('/'));
    }
    if !sub.is_empty() {
        if !p.is_empty() {
            p.push('/');
        }
        p.push_str(sub.trim_start_matches('/'));
    }
    if p.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", p)
    }
}

/// HTTP verbs Spring accepts in `RequestMethod`.
const HTTP_VERBS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE"];

/// `@RequestMapping(method = RequestMethod.POST)` — Spring declares `method` as
/// `RequestMethod[]`, so `{GET, POST}` maps the handler to several endpoints.
/// Returns every recognised verb, in declaration order, in either attribute order.
fn explicit_methods(anns: &[crate::java::Ann]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in anns {
        if a.name != "RequestMapping" {
            continue;
        }
        let raw = match crate::java::ann_attr_raw(a, "method") {
            Some(r) => r,
            None => continue,
        };
        let inner = raw.trim().trim_start_matches('{').trim_end_matches('}').to_string();
        for part in crate::java::split_args(&inner) {
            let verb = part.trim().rsplit('.').next().unwrap_or("").trim().to_uppercase();
            if HTTP_VERBS.contains(&verb.as_str()) && !out.contains(&verb) {
                out.push(verb);
            }
        }
    }
    out
}

const MAPPING_ANNOS: &[(&str, &str)] = &[
    ("GetMapping", "GET"),
    ("PostMapping", "POST"),
    ("PutMapping", "PUT"),
    ("DeleteMapping", "DELETE"),
    ("PatchMapping", "PATCH"),
];

/// Splits a parameter list on top-level commas (respecting <> nesting).
fn split_params(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth_angle = 0i32;
    let mut depth_paren = 0i32;
    let mut cur = String::new();
    for c in text.chars() {
        match c {
            '<' => { depth_angle += 1; cur.push(c); }
            '>' => { depth_angle -= 1; cur.push(c); }
            '(' => { depth_paren += 1; cur.push(c); }
            ')' => { depth_paren -= 1; cur.push(c); }
            ',' if depth_angle == 0 && depth_paren == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// The parameter's declared name = last identifier after annotations and generics.
fn param_name_decl(decl: &str) -> Option<(String, String)> {
    // strip annotations `@Foo("x") bar Baz name` -> work on scan-like simplified text:
    // remove @... and its args conservatively.
    let mut cleaned = String::new();
    let chars: Vec<char> = decl.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            // skip to end of identifier and optional (..)
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && chars[i] == '(' {
                let mut d = 0;
                while i < chars.len() {
                    if chars[i] == '(' { d += 1; }
                    if chars[i] == ')' { d -= 1; if d == 0 { i += 1; break; } }
                    i += 1;
                }
            }
            cleaned.push(' ');
        } else {
            cleaned.push(chars[i]);
            i += 1;
        }
    }
    let t = cleaned.trim();
    // remove `final`
    let t = t.split_whitespace().filter(|w| *w != "final").collect::<Vec<_>>().join(" ");
    // name = last identifier token
    let name = t.rsplit(' ').next()?.trim().to_string();
    if name.is_empty() || is_keyword(&name) {
        return None;
    }
    let mut ty = t[..t.len().saturating_sub(name.len())].trim().to_string();
    // `String... args` == `String[] args`
    if ty.ends_with("...") {
        ty = format!("{}[]", ty.trim_end_matches('.'));
    }
    if ty.is_empty() {
        return None;
    }
    Some((name, ty))
}

/// Reads `attr = "v"` / bare `"v"` from a param annotation's args (raw Java text with strings).
fn ann_attr_string(args: &str, attr: &str) -> Option<String> {
    crate::java::ann_string(&crate::java::Ann { name: String::new(), args: args.to_string() }, attr)
}

/// The wire name of `@RequestParam` / `@PathVariable` / `@RequestHeader`:
/// `value = "x"`, `name = "x"`, or the shorthand `@RequestParam("x")`.
/// `None` means "use the Java variable name".
fn param_alias(args: &str) -> Option<String> {
    let ann = crate::java::Ann { name: String::new(), args: args.to_string() };
    if let Some(v) = crate::java::ann_string(&ann, "value") {
        return Some(v);
    }
    if let Some(v) = crate::java::ann_string(&ann, "name") {
        return Some(v);
    }
    // Shorthand `@RequestParam("q")`: only when no `attr = value` form exists,
    // otherwise the first string could be `defaultValue = "..."`.
    if !args.contains('=') {
        return crate::java::first_string(args);
    }
    None
}

/// Parses one controller method into one endpoint per declared HTTP method.
/// Empty when the method carries no mapping.
pub fn parse_endpoints(cls: &JavaClass, m: &MethodRaw) -> Vec<Endpoint> {
    let mut methods: Vec<String> = Vec::new();
    let mut sub_path = String::new();
    for a in &m.anns {
        for (name, verb) in MAPPING_ANNOS {
            if &a.name == name {
                if methods.is_empty() {
                    methods.push(verb.to_string());
                }
                sub_path = ann_path(a).unwrap_or_default();
            }
        }
        if a.name == "RequestMapping" {
            if sub_path.is_empty() {
                sub_path = ann_path(a).unwrap_or_default();
            }
            if methods.is_empty() {
                methods = explicit_methods(&m.anns);
            }
        }
    }
    if methods.is_empty() {
        return Vec::new();
    }
    let path = join_path(&base_path(cls), &sub_path);

    let jd = m.javadoc.as_ref();
    let summary = jd
        .map(|j| j.desc.lines().next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            // @ApiOperation / @Operation fallback
            for a in &m.anns {
                if a.name == "ApiOperation" || a.name == "Operation" {
                    if let Some(v) = ann_path(a) {
                        return Some(v);
                    }
                }
            }
            None
        })
        .unwrap_or_else(|| m.name.clone());
    let description = jd
        .map(|j| {
            j.desc
                .lines()
                .skip(1)
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string()
        })
        .unwrap_or_default();

    let mut ep = Endpoint {
        method: methods[0].clone(),
        path,
        controller: cls.name.clone(),
        controller_desc: cls
            .javadoc
            .as_ref()
            .map(|j| j.desc.lines().next().unwrap_or("").trim().to_string())
            .unwrap_or_default(),
        handler: m.name.clone(),
        summary,
        description,
        ..Default::default()
    };

    // ---- parameters ----
    let jd_params = jd.map(|j| &j.params).cloned().unwrap_or_default();
    let jd_get = |name: &str| -> String {
        jd_params
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| d.trim().to_string())
            .unwrap_or_default()
    };

    let pieces = split_params(&m.params_text);
    for piece in &pieces {
        let piece_anns = collect_param_anns(piece);
        let (name, ty) = match param_name_decl(piece) {
            Some(x) => x,
            None => continue,
        };

        let mut location: Option<String> = None;
        let mut required: Option<bool> = None;
        let mut default: Option<String> = None;
        let mut body_type: Option<String> = None;
        let mut alias: Option<String> = None;

        for (an, args) in &piece_anns {
            match an.as_str() {
                "RequestParam" => {
                    location = Some("query".into());
                    alias = param_alias(args);
                    if args.contains("required = false") || args.contains("required=false") {
                        required = Some(false);
                    }
                    if let Some(d) = ann_attr_string(args, "defaultValue") {
                        default = Some(d);
                        required = Some(false);
                    }
                }
                "PathVariable" => {
                    location = Some("path".into());
                    alias = param_alias(args);
                }
                "RequestHeader" => {
                    location = Some("header".into());
                    alias = param_alias(args);
                }
                "RequestBody" => {
                    body_type = Some(ty.clone());
                    if args.contains("required = false") || args.contains("required=false") {
                        required = Some(false);
                    }
                }
                "ModelAttribute" | "ParameterObject" => {
                    // binds query params from DTO fields; expanded later
                    location = Some("object".into());
                }
                _ => {}
            }
        }

        if let Some(bt) = body_type {
            ep.request_body = Some(bt);
            ep.request_desc = jd_get(&name);
            continue;
        }

        let loc = location.unwrap_or_else(|| {
            // Spring binds simple unannotated params from the query string
            if is_simple_type(&ty) { "query".to_string() } else { "object".to_string() }
        });

        // Spring binds by the annotation alias when one is given; the Java
        // variable name is only a fallback. Javadoc @param always documents
        // the variable name, so the description is looked up with `name`.
        let wire_name = alias.clone().unwrap_or_else(|| name.clone());
        let mut p = Param {
            name: wire_name,
            location: loc,
            java_type: ty.clone(),
            required: required.unwrap_or(true),
            default,
            desc: jd_get(&name),
        };
        if p.location == "query" && p.java_type.contains("Map") && p.java_type.contains('<') {
            // Map params are untyped query bags
            p.java_type = "object".into();
        }
        ep.params.push(p);
    }

    // path variables not declared with @PathVariable (e.g. mapped implicitly)
    for v in path_variables(&ep.path) {
        if !ep.params.iter().any(|p| p.name == v) {
            ep.params.push(Param {
                name: v.clone(),
                location: "path".into(),
                java_type: "String".into(),
                required: true,
                default: None,
                desc: jd_get(&v),
            });
        } else {
            for p in ep.params.iter_mut() {
                if p.name == v {
                    p.location = "path".into();
                    p.required = true;
                }
            }
        }
    }

    // ---- response ----
    let (envelope, payload) = response_shape(&m.return_type);
    ep.response_envelope = envelope;
    ep.response = payload;
    ep.return_desc = jd.map(|j| j.ret.clone()).unwrap_or_default();

    // `method = {GET, POST}` yields one endpoint per verb, sharing everything else.
    methods
        .iter()
        .enumerate()
        .map(|(i, verb)| {
            let mut e = ep.clone();
            if i > 0 {
                e.method = verb.clone();
            }
            e
        })
        .collect()
}

/// First endpoint of `parse_endpoints`; None when the method has no mapping.
pub fn parse_endpoint(cls: &JavaClass, m: &MethodRaw) -> Option<Endpoint> {
    parse_endpoints(cls, m).into_iter().next()
}

fn collect_param_anns(piece: &str) -> Vec<(String, String)> {
    // crude: find @Name(...) or @Name at top level
    let mut out = Vec::new();
    let chars: Vec<char> = piece.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[start + 1..i].iter().collect();
            let mut args = String::new();
            if i < chars.len() && chars[i] == '(' {
                let mut d = 0;
                let arg_start = i + 1;
                while i < chars.len() {
                    if chars[i] == '(' { d += 1; }
                    if chars[i] == ')' {
                        d -= 1;
                        if d == 0 { i += 1; break; }
                    }
                    i += 1;
                }
                args = chars[arg_start..i - 1].iter().collect();
            }
            out.push((name, args));
        } else {
            i += 1;
        }
    }
    out
}

/// Java type -> simple class name (drops package, generics, array suffix).
pub fn simple_type_name(t: &str) -> String {
    let base = t.split('<').next().unwrap_or(t).trim();
    let base = base.trim_end_matches("[]").trim();
    base.rsplit('.').next().unwrap_or(base).trim().to_string()
}

fn is_simple_type(t: &str) -> bool {
    let base = simple_type_name(t);
    matches!(
        base.as_str(),
        "String" | "int" | "Integer" | "long" | "Long" | "short" | "Short"
            | "double" | "Double" | "float" | "Float" | "boolean" | "Boolean"
            | "BigDecimal" | "BigInteger" | "Date" | "LocalDate" | "LocalDateTime"
            | "LocalTime" | "MultipartFile" | "char" | "Character" | "byte" | "Byte"
    )
}

/// Servlet / framework parameters that carry no API meaning.
const FRAMEWORK_TYPES: &[&str] = &[
    "HttpServletRequest", "HttpServletResponse", "ServletRequest", "ServletResponse",
    "BindingResult", "Errors", "Model", "ModelMap", "HttpSession", "Principal",
    "Locale", "TimeZone", "SessionStatus", "RedirectAttributes", "UriComponentsBuilder",
    "HttpHeaders", "WebRequest", "NativeWebRequest", "MultipartHttpServletRequest",
    "InputStream", "OutputStream", "Reader", "Writer",
];

fn is_framework_type(t: &str) -> bool {
    FRAMEWORK_TYPES.contains(&simple_type_name(t).as_str())
}

/// True for `List<...>` / `Set<...>` / `Collection<...>` / `Map<...>`.
fn is_collection_type(t: &str) -> bool {
    matches!(simple_type_name(t).as_str(), "List" | "Set" | "Collection" | "Map" | "ArrayList" | "HashMap")
}

/// Transport wrappers that say nothing about the payload.
const TRANSPORT_WRAPPERS: &[&str] = &["ResponseEntity", "HttpEntity"];

/// Result envelopes: bare `<Name>` or `<Name><T>`.
pub const ENVELOPES: &[&str] = &[
    "R",
    "Result",
    "AjaxResult",
    "CommonResult",
    "BaseResponse",
    "ApiResponse",
    "ResponseResult",
    "RespResult",
    "ResultData",
];

/// `t<args>` when `t` is exactly `name<...>`; returns the inner text.
fn generic_inner(t: &str, name: &str) -> Option<String> {
    let t = t.trim();
    if !t.ends_with('>') {
        return None;
    }
    let open = t.find('<')?;
    if simple_type_name(&t[..open]) != name {
        return None;
    }
    let mut depth = 0i32;
    for (off, c) in t[open..].char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    // the outermost generic must close at the very end
                    return if open + off + c.len_utf8() == t.len() {
                        Some(t[open + 1..t.len() - 1].trim().to_string())
                    } else {
                        None
                    };
                }
            }
            _ => {}
        }
    }
    None
}

fn is_void(t: &str) -> bool {
    matches!(simple_type_name(t).as_str(), "" | "void" | "Void")
}

/// Splits a return type into (envelope, payload).
/// `ResponseEntity<R<IPage<UserVo>>>` -> (Some("R"), Some("IPage<UserVo>"))
/// `List<UserVo>` -> (None, Some("List<UserVo>"))
/// `R` -> (Some("R"), None); `void` -> (None, None)
pub fn response_shape(return_type: &str) -> (Option<String>, Option<String>) {
    let mut t = return_type.trim().to_string();
    loop {
        let mut stripped = false;
        for w in TRANSPORT_WRAPPERS {
            if let Some(inner) = generic_inner(&t, w) {
                t = inner;
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }
    for w in ENVELOPES {
        if let Some(inner) = generic_inner(&t, w) {
            let payload = if is_void(&inner) { None } else { Some(inner) };
            return (Some((*w).to_string()), payload);
        }
    }
    let b = simple_type_name(&t);
    if ENVELOPES.contains(&b.as_str()) {
        return (Some(b), None);
    }
    if is_void(&t) {
        return (None, None);
    }
    (None, Some(t))
}

/// Conventional shape of a result envelope when its source is not available.
pub fn envelope_shape(name: &str) -> Option<Schema> {
    if !ENVELOPES.contains(&name) {
        return None;
    }
    Some(Schema {
        name: name.to_string(),
        desc: format!("统一响应结构 {}（未找到类源码，按约定推断）", name),
        fields: vec![
            FieldDoc { name: "code".into(), java_type: "int".into(), desc: "状态码（0 表示成功）".into(), required: true },
            FieldDoc { name: "msg".into(), java_type: "String".into(), desc: "提示信息".into(), required: false },
            FieldDoc { name: "data".into(), java_type: "Object".into(), desc: "业务数据".into(), required: false },
        ],
    })
}

/// Expands object-bound parameters (`@ParameterObject`, `@ModelAttribute`,
/// unannotated DTO args) into individual query parameters from the DTO fields.
pub fn expand_object_params(
    endpoints: &mut [Endpoint],
    lookup: &std::collections::BTreeMap<String, JavaClass>,
) {
    for ep in endpoints.iter_mut() {
        let objects: Vec<Param> = ep
            .params
            .iter()
            .filter(|p| p.location == "object")
            .cloned()
            .collect();
        if objects.is_empty() {
            continue;
        }
        ep.params.retain(|p| p.location != "object");
        for p in objects {
            if is_framework_type(&p.java_type) {
                continue;
            }
            if is_collection_type(&p.java_type) {
                // `Map<String,Object> params` is a free-form query bag; other
                // collections cannot be expanded into named query parameters.
                if simple_type_name(&p.java_type) == "Map" {
                    ep.params.push(Param {
                        name: p.name.clone(),
                        location: "query".into(),
                        java_type: "object".into(),
                        required: p.required,
                        default: p.default.clone(),
                        desc: p.desc.clone(),
                    });
                }
                continue;
            }
            let base = simple_type_name(&p.java_type);
            match lookup.get(&base) {
                Some(cls) => {
                    for f in schema_for_class(cls).fields {
                        if ep.params.iter().any(|q| q.name == f.name) {
                            continue; // an explicit @RequestParam already covers it
                        }
                        ep.params.push(Param {
                            name: f.name,
                            location: "query".into(),
                            java_type: if f.java_type.is_empty() { "String".into() } else { f.java_type },
                            required: f.required,
                            default: None,
                            desc: f.desc,
                        });
                    }
                }
                None => {
                    let note = format!("（未找到类型 {} 的源码，按对象参数展示）", base);
                    let desc = if p.desc.is_empty() { note } else { format!("{}{}", p.desc, note) };
                    ep.params.push(Param {
                        name: p.name.clone(),
                        location: "query".into(),
                        java_type: "object".into(),
                        required: p.required,
                        default: p.default.clone(),
                        desc,
                    });
                }
            }
        }
    }
}

/// Builds a Schema from a parsed DTO class (fields + Javadoc + validation annotations).
pub fn schema_for_class(cls: &JavaClass) -> Schema {
    let mut s = Schema {
        name: cls.name.clone(),
        desc: cls
            .javadoc
            .as_ref()
            .map(|j| j.desc.lines().next().unwrap_or("").trim().to_string())
            .unwrap_or_default(),
        fields: Vec::new(),
    };
    for f in &cls.fields {
        // constants such as `serialVersionUID` are not part of the payload
        if f.is_static || f.name == "serialVersionUID" {
            continue;
        }
        let mut desc = f
            .javadoc
            .as_ref()
            .map(|j| j.desc.trim().to_string())
            .unwrap_or_default();
        if desc.is_empty() {
            for a in &f.anns {
                if a.name == "ApiModelProperty" || a.name == "Schema" || a.name == "Schema" {
                    if let Some(v) = ann_path(a) {
                        desc = v;
                        break;
                    }
                }
                if a.name == "ExcelProperty" {
                    if let Some(v) = ann_path(a) {
                        if desc.is_empty() {
                            desc = v;
                        }
                    }
                }
            }
        }
        let primitive = matches!(
            simple_type_name(&f.java_type).as_str(),
            "int" | "long" | "short" | "double" | "float" | "boolean" | "byte" | "char"
        );
        let required = primitive
            || f.anns
                .iter()
                .any(|a| matches!(a.name.as_str(), "NotNull" | "NotEmpty" | "NotBlank"));
        // @JsonProperty rename
        let mut name = f.name.clone();
        for a in &f.anns {
            if a.name == "JsonProperty" {
                if let Some(v) = ann_path(a) {
                    name = v;
                }
            }
        }
        s.fields.push(FieldDoc { name, java_type: f.java_type.clone(), desc, required });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::JavaFile;

    #[test]
    fn parses_controller_endpoints() {
        let src = r#"
        /** 用户管理 */
        @RestController
        @RequestMapping("/api/user")
        public class UserController {
            /** 分页查询用户 */
            @GetMapping("/list")
            public Result<List<UserVo>> list(UserQuery q) { return null; }

            /**
             * 创建用户
             * 创建一个新用户并返回ID
             * @param dto 用户信息
             * @return 用户ID
             */
            @PostMapping("/create")
            public Result<Long> create(@RequestBody UserDto dto) { return null; }

            @GetMapping("/detail/{id}")
            public Result<UserVo> detail(@PathVariable("id") Long id) { return null; }
        }
        "#;
        let f = JavaFile::parse(src);
        let cls = &f.classes[0];
        assert_eq!(cls.name, "UserController");
        let eps: Vec<Endpoint> = cls.methods.iter().filter_map(|m| parse_endpoint(cls, m)).collect();
        assert_eq!(eps.len(), 3);
        assert_eq!(eps[0].method, "GET");
        assert_eq!(eps[0].path, "/api/user/list");
        assert_eq!(eps[0].summary, "分页查询用户");
        assert_eq!(eps[1].method, "POST");
        assert_eq!(eps[1].request_body.as_deref(), Some("UserDto"));
        assert_eq!(eps[1].request_desc, "用户信息");
        assert_eq!(eps[1].return_desc, "用户ID");
        assert_eq!(eps[2].path, "/api/user/detail/{id}");
        assert_eq!(eps[2].params[0].location, "path");
    }

    #[test]
    fn dto_schema_has_comments() {
        let src = r#"
        /** 用户传输对象 */
        @Data
        public class UserDto {
            /** 用户名 */
            @NotNull
            private String username;
            /** 年龄 */
            private Integer age;
            /** 手机号 */
            private String mobile;
        }
        "#;
        let f = JavaFile::parse(src);
        let s = schema_for_class(&f.classes[0]);
        assert_eq!(s.desc, "用户传输对象");
        assert_eq!(s.fields[0].desc, "用户名");
        assert!(s.fields[0].required);
        assert_eq!(s.fields[1].java_type, "Integer");
    }

    #[test]
    fn response_unwraps_generics() {
        assert_eq!(
            response_shape("ResponseEntity<List<UserVo>>"),
            (None, Some("List<UserVo>".to_string()))
        );
        assert_eq!(
            response_shape("R<IPage<UserVo>>"),
            (Some("R".to_string()), Some("IPage<UserVo>".to_string()))
        );
        assert_eq!(response_shape("R<Boolean>"), (Some("R".to_string()), Some("Boolean".to_string())));
        assert_eq!(response_shape("R"), (Some("R".to_string()), None));
        assert_eq!(response_shape("void"), (None, None));
        assert_eq!(response_shape("UserVo"), (None, Some("UserVo".to_string())));
    }

    #[test]
    fn class_name_ignores_generic_superclass() {
        let src = r#"
        @Data
        public class SysAreaEntity extends Model<SysAreaEntity> {
            /** 名称 */
            private String name;
        }
        "#;
        let f = JavaFile::parse(src);
        assert_eq!(f.classes[0].name, "SysAreaEntity");
        assert_eq!(f.classes[0].fields[0].name, "name");
    }

    #[test]
    fn array_and_varargs_params_are_query() {
        let src = r#"
        @RestController
        public class C {
            @PostMapping("/export")
            public void export(String[] ids, Long... more) {}
        }
        "#;
        let f = JavaFile::parse(src);
        let cls = &f.classes[0];
        let ep = parse_endpoint(cls, &cls.methods[0]).unwrap();
        assert_eq!(ep.params[0].location, "query");
        assert_eq!(ep.params[0].java_type, "String[]");
        assert_eq!(ep.params[1].java_type, "Long[]");
    }

    #[test]
    fn object_params_expand_to_dto_fields() {
        let src = r#"
        @RestController
        public class C {
            @GetMapping("/page")
            public R page(@ParameterObject SysAreaEntity sysArea, HttpServletResponse resp) {}
        }
        /** 行政区划表 */
        @Data
        public class SysAreaEntity {
            /** 名称 */
            @NotBlank
            private String name;
            /** 层级 */
            private Integer level;
        }
        "#;
        let f = JavaFile::parse(src);
        let mut eps: Vec<Endpoint> = vec![parse_endpoint(&f.classes[0], &f.classes[0].methods[0]).unwrap()];
        let mut lookup = std::collections::BTreeMap::new();
        for c in &f.classes {
            lookup.insert(c.name.clone(), c.clone());
        }
        expand_object_params(&mut eps, &lookup);
        let names: Vec<&str> = eps[0].params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["name", "level"]);
        assert_eq!(eps[0].params[0].desc, "名称");
        assert!(eps[0].params[0].required);
    }

    #[test]
    fn request_mapping_enum_method_in_both_orders() {
        let src = r#"
        @RestController
        public class C {
            @RequestMapping(value = "/users", method = RequestMethod.POST)
            public String create() { return "ok"; }

            @RequestMapping(method = RequestMethod.POST, value = "/users")
            public String createAgain() { return "ok"; }

            @RequestMapping(value = "/multi", method = {RequestMethod.GET, RequestMethod.POST})
            public String multi() { return "ok"; }
        }
        "#;
        let f = JavaFile::parse(src);
        let cls = &f.classes[0];
        let eps: Vec<Endpoint> = cls.methods.iter().flat_map(|m| parse_endpoints(cls, m)).collect();
        assert_eq!(eps.len(), 4);
        assert_eq!((eps[0].method.as_str(), eps[0].path.as_str()), ("POST", "/users"));
        assert_eq!((eps[1].method.as_str(), eps[1].path.as_str()), ("POST", "/users"));
        assert_eq!((eps[2].method.as_str(), eps[2].path.as_str()), ("GET", "/multi"));
        assert_eq!((eps[3].method.as_str(), eps[3].path.as_str()), ("POST", "/multi"));
    }

    #[test]
    fn explicit_param_names_are_honored() {
        let src = r#"
        @RestController
        public class C {
            @GetMapping("/users/{id}")
            public String detail(
                @PathVariable("id") Long userId,
                @RequestParam(name = "q") String keyword,
                @RequestHeader("X-Trace-Id") String traceId,
                @RequestParam(value = "page", defaultValue = "1") int pageNo) { return "ok"; }
        }
        "#;
        let f = JavaFile::parse(src);
        let cls = &f.classes[0];
        let ep = parse_endpoint(cls, &cls.methods[0]).unwrap();
        let got: Vec<(&str, &str, &str)> = ep
            .params
            .iter()
            .map(|p| (p.name.as_str(), p.location.as_str(), p.java_type.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("id", "path", "Long"),
                ("q", "query", "String"),
                ("X-Trace-Id", "header", "String"),
                ("page", "query", "int"),
            ]
        );
        assert_eq!(ep.params[3].default.as_deref(), Some("1"));
    }
}
