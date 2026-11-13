//! Spring Api Export: exports Spring Boot controller API docs
//! (Markdown / OpenAPI JSON), including Javadoc and DTO field comments,
//! and imports selected endpoints into an Apifox project.

use openidea_plugin::{
    host, Error, Invocation, NotificationLevel, PickItem, Plugin, Project, register_plugin,
};

#[cfg(debug_assertions)]
pub mod java;
#[cfg(debug_assertions)]
pub mod markdown;
#[cfg(debug_assertions)]
pub mod model;
#[cfg(debug_assertions)]
pub mod openapi;
#[cfg(debug_assertions)]
pub mod spring;

#[cfg(not(debug_assertions))]
mod java;
#[cfg(not(debug_assertions))]
mod markdown;
#[cfg(not(debug_assertions))]
mod model;
#[cfg(not(debug_assertions))]
mod openapi;
#[cfg(not(debug_assertions))]
mod spring;

mod apifox;

const TITLE: &str = "Spring Api Export";
/// Hard cap on scanned .java files (enough for any sane service project).
const MAX_FILES: u32 = 20000;
const MARKDOWN_PATH: &str = "docs/api.md";
const OPENAPI_PATH: &str = "docs/openapi.json";
/// Declared in `[[contributes.secrets]]`; the host fills it into requests.
const APIFOX_SECRET: &str = "apifox_token";
/// Storage key prefix; the project root is appended so each IDE project keeps
/// its own Apifox project id.
const APIFOX_PROJECT_KEY_PREFIX: &str = "apifox.project_id:";

#[derive(Default)]
pub struct Main;

impl Plugin for Main {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "export-markdown" | "export-openapi" => self.export(id, ctx),
            "export-apifox" => self.export_apifox(ctx),
            "configure-apifox" => self.configure_apifox(ctx),
            _ => Err(format!("unknown command {id}").into()),
        }
    }
}

impl Main {
    fn export(&self, id: &str, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("请先打开一个 Spring Boot 项目再运行该命令")?;
        let selected = ctx.selected_paths();
        let (mut endpoints, resolver) = if selected.is_empty() {
            scan(&project)?
        } else {
            scan_selected(&project, &selected)?
        };
        if endpoints.is_empty() {
            return Err(if selected.is_empty() {
                "未找到 Spring Controller（已扫描项目中所有 .java 文件的 @RestController/@Controller）".into()
            } else {
                "选中的文件/目录里没有找到 Spring Controller（@RestController/@Controller）".into()
            });
        }
        spring::expand_object_params(&mut endpoints, &resolver.lookup);
        let refs = resolver.resolve(&endpoints);
        let (path, content, label) = if id == "export-markdown" {
            let md = markdown::render(&endpoints, &refs);
            (MARKDOWN_PATH, md, "Markdown")
        } else {
            let writer = openapi::OpenApiWriter { title: "API 文档".into() };
            (OPENAPI_PATH, writer.render(&endpoints, &refs), "OpenAPI JSON")
        };
        project.write_file(path, &content)?;
        let summary = format!("已导出 {} 个接口到 {path}", endpoints.len());
        host::notify(NotificationLevel::Info, TITLE, &summary);
        Ok(Some(format!("{label}: {summary}")))
    }

    /// Project-view context menu: parse the selected controller file(s) or
    /// folder (whole project when nothing is selected), let the user tick the
    /// endpoints to publish, then import them into Apifox.
    fn export_apifox(&self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("请先打开一个 Spring Boot 项目再运行该命令")?;
        let selected = ctx.selected_paths();
        let (mut endpoints, resolver) = if selected.is_empty() {
            scan(&project)?
        } else {
            scan_selected(&project, &selected)?
        };
        if endpoints.is_empty() {
            return Err(if selected.is_empty() {
                "未找到 Spring Controller（已扫描项目中所有 .java 文件）".into()
            } else {
                "选中的文件/目录里没有找到 Spring Controller（@RestController/@Controller）".into()
            });
        }
        spring::expand_object_params(&mut endpoints, &resolver.lookup);

        // Let the user pick which endpoints to publish. The keywords are padded
        // so the paths line up, and the controller prefix is dropped when every
        // candidate comes from the same controller (the common case).
        const ALL_KEY: &str = "all";
        let single_controller = endpoints
            .iter()
            .all(|ep| ep.controller == endpoints[0].controller);
        let mut items: Vec<PickItem> = Vec::with_capacity(endpoints.len() + 1);
        items.push(
            PickItem::new(ALL_KEY, &format!("全选 · 导出全部 {} 个接口", endpoints.len()))
                .description("勾选这一项即可导出全部接口，不必再逐个勾选"),
        );
        for (i, ep) in endpoints.iter().enumerate() {
            let mut label = format!("{:<6} {}", ep.method, ep.path);
            if !ep.summary.is_empty() {
                label.push_str(" · ");
                label.push_str(&ep.summary);
            }
            let desc = if single_controller {
                ep.handler.clone()
            } else {
                format!("{}#{}", ep.controller, ep.handler)
            };
            items.push(PickItem::new(&i.to_string(), &label).description(&desc));
        }
        let Some(chosen) = host::pick_many("选择要导出到 Apifox 的接口", &items)? else {
            return Ok(None); // dismissed
        };
        let mut picked = Vec::new();
        if chosen.iter().any(|key| key == ALL_KEY) {
            picked = endpoints.clone();
        } else {
            for key in &chosen {
                if let Ok(i) = key.parse::<usize>() {
                    if let Some(ep) = endpoints.get(i) {
                        picked.push(ep.clone());
                    }
                }
            }
        }
        if picked.is_empty() {
            return Ok(Some("未选择任何接口，未导出".into()));
        }

        let refs = resolver.resolve(&picked);
        let json = openapi::OpenApiWriter { title: "API 文档".into() }.render(&picked, &refs);

        let Some(project_id) = resolve_project_id(&project)? else {
            return Ok(None);
        };
        if !ensure_token()? {
            return Ok(None);
        }

        let outcome = apifox::import(&project_id, &json)?;
        let mut message = format!("已导出 {} 个接口到 Apifox：{}", picked.len(), outcome.summary());
        if !outcome.errors.is_empty() {
            message.push_str("；Apifox 提示：");
            message.push_str(&outcome.errors.join("；"));
            host::notify(NotificationLevel::Warning, TITLE, &message);
        } else {
            host::notify(NotificationLevel::Info, TITLE, &message);
        }
        Ok(Some(message))
    }

    /// Lets the user change the two Apifox settings without leaving the editor:
    /// the per-project id (storage) and the global access token (keychain).
    fn configure_apifox(&self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("请先打开一个 Spring Boot 项目再运行该命令")?;
        let root = project.root().unwrap_or_default();
        let key = format!("{APIFOX_PROJECT_KEY_PREFIX}{root}");
        let current = host::storage::get(&key).ok().flatten().unwrap_or_default();
        let current = current.trim().to_string();
        let prompt = if current.is_empty() {
            "Apifox 项目 ID（数字，可在 Apifox 项目设置中查看；留空跳过）".to_string()
        } else {
            format!("Apifox 项目 ID（当前 {current}；改成新数字即覆盖，清空后确定即删除）")
        };
        let mut notes: Vec<String> = Vec::new();
        let default = if current.is_empty() { None } else { Some(current.as_str()) };
        if let Some(raw) = host::input(&prompt, default)? {
            let value = raw.trim();
            if value.is_empty() {
                if !current.is_empty() {
                    host::storage::delete(&key)?;
                    notes.push(format!("已删除项目 ID（原 {current}），下次导出会重新询问"));
                }
            } else if !value.chars().all(|c| c.is_ascii_digit()) {
                return Err("Apifox 项目 ID 必须是数字".into());
            } else if value != current {
                host::storage::set(&key, value)?;
                notes.push(format!("项目 ID 已设为 {value}"));
            }
        }

        let has_token = host::secret_is_set(APIFOX_SECRET);
        let body = if has_token {
            "选「是」可以替换或清除已保存的令牌，选「否」保持不变"
        } else {
            "选「是」现在填写令牌，选「否」跳过（下次导出时再问）"
        };
        if host::confirm("是否重新设置 Apifox 访问令牌？", body)? {
            let token_prompt = if has_token {
                "输入新的 Apifox 访问令牌（留空并确定 = 删除已保存的令牌）"
            } else {
                "输入 Apifox 访问令牌（留空跳过）"
            };
            if let Some(raw) = host::input(token_prompt, None)? {
                let raw = raw.trim();
                let token = raw.strip_prefix("Bearer ").unwrap_or(raw).trim();
                if token.is_empty() {
                    if has_token {
                        host::secret_delete(APIFOX_SECRET)?;
                        notes.push("已删除访问令牌，下次导出会重新询问".into());
                    }
                } else {
                    host::secret_set(APIFOX_SECRET, token)?;
                    notes.push("访问令牌已更新".into());
                }
            }
        }

        let message = if notes.is_empty() {
            "Apifox 设置未改动".to_string()
        } else {
            notes.join("；")
        };
        host::notify(NotificationLevel::Info, TITLE, &message);
        Ok(Some(message))
    }
}

/// Reads the per-project Apifox project id from storage, asking once and
/// validating it as a number. `None` means the user dismissed the prompt.
fn resolve_project_id(project: &Project) -> Result<Option<String>, Error> {
    let root = project.root().unwrap_or_default();
    let key = format!("{APIFOX_PROJECT_KEY_PREFIX}{root}");
    if let Ok(Some(saved)) = host::storage::get(&key) {
        let saved = saved.trim().to_string();
        if !saved.is_empty() {
            return Ok(Some(saved));
        }
    }
    let Some(answer) = host::input("Apifox 项目 ID（数字，可在 Apifox 项目设置中查看）", None)? else {
        return Ok(None);
    };
    let answer = answer.trim().to_string();
    if answer.is_empty() {
        return Ok(None);
    }
    if !answer.chars().all(|c| c.is_ascii_digit()) {
        return Err("Apifox 项目 ID 必须是数字".into());
    }
    host::storage::set(&key, &answer)?;
    Ok(Some(answer))
}

/// Makes sure the access token is in the keychain; asks for it once.
/// Returns `false` when the user dismissed the prompt.
fn ensure_token() -> Result<bool, Error> {
    if host::secret_is_set(APIFOX_SECRET) {
        return Ok(true);
    }
    let Some(answer) = host::input(
        "Apifox 访问令牌（Apifox › 个人设置 › 开放 API 中生成的令牌）",
        None,
    )?
    else {
        return Ok(false);
    };
    let token = answer.trim();
    let token = token.strip_prefix("Bearer ").unwrap_or(token).trim();
    if token.is_empty() {
        return Ok(false);
    }
    host::secret_set(APIFOX_SECRET, token)?;
    Ok(true)
}

/// Scans every .java file: collects controller endpoints and all classes
/// (for DTO schema resolution) in one pass.
fn scan(project: &Project) -> Result<(Vec<model::Endpoint>, markdown::DtoResolver), Error> {
    let root = project.root().unwrap_or_default();
    let files = project.find_files("**/*.java", MAX_FILES)?;
    parse_files(project, &root, &files, None)
}

/// Scans the whole project for DTO classes but only collects endpoints from the
/// given project-view selection (a `.java` file or a folder). Resolving DTOs
/// from the whole project is what keeps request/response schemas from dangling.
fn scan_selected(
    project: &Project,
    selected: &[String],
) -> Result<(Vec<model::Endpoint>, markdown::DtoResolver), Error> {
    let root = project.root().unwrap_or_default();
    let files = project.find_files("**/*.java", MAX_FILES)?;
    let sels: Vec<String> = selected
        .iter()
        .map(|p| normalize_path(&relative_to_root(&root, p)))
        .filter(|p| !p.is_empty())
        .collect();
    parse_files(project, &root, &files, Some(&sels))
}

/// Forward-slash, no leading slash.
fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_string()
}

/// True when `rel` (root-relative .java path) is the selected file or inside
/// the selected folder.
fn matches_selection(rel: &str, sel: &str) -> bool {
    let sel = sel.trim_end_matches('/');
    if sel.to_ascii_lowercase().ends_with(".java") {
        return rel.eq_ignore_ascii_case(sel);
    }
    rel.eq_ignore_ascii_case(sel)
        || rel
            .to_ascii_lowercase()
            .starts_with(&format!("{}/", sel.to_ascii_lowercase()))
}

/// Turns an absolute project path into a root-relative, forward-slash path.
fn relative_to_root(root: &str, path: &str) -> String {
    let root = root.replace('\\', "/");
    let path = path.replace('\\', "/");
    let trimmed_root = root.trim_end_matches('/');
    if !trimmed_root.is_empty() {
        if let Some(rest) = path.strip_prefix(trimmed_root) {
            return rest.trim_start_matches('/').to_string();
        }
    }
    path.trim_start_matches('/').to_string()
}

fn parse_files(
    project: &Project,
    root: &str,
    files: &[String],
    only: Option<&[String]>,
) -> Result<(Vec<model::Endpoint>, markdown::DtoResolver), Error> {
    let task = host::begin_task("扫描 Spring Controller", false);
    let mut endpoints = Vec::new();
    let mut resolver = markdown::DtoResolver::new();
    let total = files.len() as u32;
    for (i, path) in files.iter().enumerate() {
        if task.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if i % 50 == 0 {
            task.report(i as u32, Some(total), Some(path.as_str()));
        }
        let src = match project.read_file(path) {
            Ok(s) => s,
            // skip unreadable files (deleted, non-UTF8 wrapper rejects, ...)
            Err(_) => continue,
        };
        let take_endpoints = match only {
            None => true,
            Some(sels) => {
                let rel = normalize_path(&relative_to_root(root, path));
                sels.iter().any(|s| matches_selection(&rel, s))
            }
        };
        let parsed = java::JavaFile::parse(&src);
        for cls in &parsed.classes {
            if take_endpoints && spring::is_controller(cls) {
                for m in &cls.methods {
                    if let Some(ep) = spring::parse_endpoint(cls, m) {
                        endpoints.push(ep);
                    }
                }
            }
            resolver.add_class(cls.clone());
        }
    }
    Ok((endpoints, resolver))
}

register_plugin!(Main);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::HttpResponse;

    const CONTROLLER: &str = r#"
package com.example.web;

/** 用户管理接口 */
@RestController
@RequestMapping("/api/users")
public class UserController {
    /** 分页查询用户 */
    @GetMapping("/page")
    public Result<PageResult<UserDto>> page(@RequestParam String keyword,
                                            @RequestParam(defaultValue = "1") int page) {
        return null;
    }

    /** 按 ID 查询用户 */
    @GetMapping("/{id}")
    public Result<UserDto> detail(@PathVariable Long id) {
        return null;
    }
}
"#;

    const DTO: &str = r#"
package com.example.web;

/** 用户传输对象 */
public class UserDto {
    /** 用户名 */
    private String username;
    /** 年龄 */
    private Integer age;
}
"#;

    const CAPS: &[&str] = &[
        "project.read",
        "project.write",
        "storage",
        "secrets",
        "net:api.apifox.com",
    ];

    fn project_host() -> MockHost {
        let host = MockHost::install().with_capabilities(CAPS);
        host.declare_secret(APIFOX_SECRET);
        host.add_file("/project/src/main/java/com/example/web/UserController.java", CONTROLLER);
        host.add_file("/project/src/main/java/com/example/web/UserDto.java", DTO);
        host
    }

    #[test]
    fn exports_markdown_and_openapi() {
        let host = project_host();
        let mut plugin = Main;

        let msg = plugin
            .run_command("export-openapi", &host.invocation(), None)
            .expect("openapi export should succeed")
            .expect("should report what it wrote");
        assert!(msg.contains("2"), "unexpected message: {msg}");
        let json = written(OPENAPI_PATH);
        let doc: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(doc["openapi"], "3.0.3");
        assert!(doc["paths"].get("/api/users/page").is_some());
        assert!(doc["paths"].get("/api/users/{id}").is_some());
        assert!(doc["components"]["schemas"]["UserDto"].is_object());

        let md = plugin
            .run_command("export-markdown", &host.invocation(), None)
            .expect("markdown export should succeed");
        assert!(md.is_some());
        let text = written(MARKDOWN_PATH);
        assert!(text.contains("分页查询用户"), "javadoc summary missing:\n{text}");
        assert!(text.contains("用户名"), "DTO field comment missing:\n{text}");
    }

    #[test]
    fn no_project_is_a_friendly_error() {
        let host = MockHost::install().with_capabilities(CAPS);
        host.set_project_root(None);
        let err = Main
            .run_command("export-openapi", &host.invocation(), None)
            .expect_err("no project should fail");
        assert!(format!("{err}").contains("请先打开"), "unexpected error: {err}");
    }

    #[test]
    fn export_apifox_imports_only_selected_endpoints() {
        let host = project_host();
        host.add_file("/project/src/main/java/com/example/web/OtherController.java", "");
        // pick_many -> project id -> token, in call order.
        host.answer_next_prompt(Some(&["1"]));
        host.answer_next_prompt(Some(&["12345"]));
        host.answer_next_prompt(Some(&["tok-abc"]));
        host.on_fetch(|_req| {
            Ok(HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: r#"{"data":{"counters":{"endpointCreated":1,"endpointUpdated":0,"endpointFailed":0,"endpointIgnored":0},"errors":[]}}"#.as_bytes().to_vec(),
            })
        });

        let msg = Main
            .run_command("export-apifox", &host.invocation(), None)
            .expect("apifox export should succeed")
            .expect("should report a summary");
        assert!(msg.contains("已导出 1 个接口"), "unexpected message: {msg}");
        assert!(msg.contains("新增 1 个接口"), "unexpected message: {msg}");

        let requests = host.requests();
        assert_eq!(requests.len(), 1);
        let req = &requests[0];
        assert_eq!(
            req.url,
            "https://api.apifox.com/v1/projects/12345/import-openapi?locale=zh-CN"
        );
        let header = |name: &str| {
            req.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(header("X-Apifox-Api-Version"), Some("2024-03-28"));
        assert_eq!(header("Authorization"), Some("Bearer tok-abc"));

        let body: serde_json::Value =
            serde_json::from_slice(req.body.as_ref().expect("body")).expect("valid body JSON");
        assert_eq!(body["options"]["endpointOverwriteBehavior"], "AUTO_MERGE");
        // `input` is the OpenAPI document as a string: only the ticked endpoint.
        let input: serde_json::Value =
            serde_json::from_str(body["input"].as_str().expect("input is a string")).expect("input JSON");
        let paths = input["paths"].as_object().expect("paths object");
        assert_eq!(paths.len(), 1, "only the selected endpoint should be exported");
        assert!(paths.contains_key("/api/users/{id}"));

        // Choices are remembered for next time.
        assert_eq!(
            host.storage().get("apifox.project_id:/project").map(String::as_str),
            Some("12345")
        );
    }

    #[test]
    fn export_apifox_select_all_exports_every_endpoint() {
        let host = project_host();
        host.answer_next_prompt(Some(&["all"])); // 全选
        host.answer_next_prompt(Some(&["777"]));
        host.answer_next_prompt(Some(&["tok-all"]));
        host.on_fetch(|_req| {
            Ok(HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: r#"{"data":{"counters":{"endpointCreated":2,"endpointUpdated":0,"endpointFailed":0,"endpointIgnored":0},"errors":[]}}"#.as_bytes().to_vec(),
            })
        });

        let msg = Main
            .run_command("export-apifox", &host.invocation(), None)
            .expect("should succeed")
            .expect("summary");
        assert!(msg.contains("已导出 2 个接口"), "unexpected message: {msg}");

        let requests = host.requests();
        let body: serde_json::Value =
            serde_json::from_slice(requests[0].body.as_ref().expect("body")).expect("valid JSON");
        let input: serde_json::Value =
            serde_json::from_str(body["input"].as_str().expect("input")).expect("input JSON");
        assert_eq!(input["paths"].as_object().expect("paths").len(), 2);
    }

    #[test]
    fn export_apifox_reuses_saved_settings_and_cancel_stops_it() {
        let host = project_host();
        host.set_secret(APIFOX_SECRET, "saved-token");
        host::storage::set("apifox.project_id:/project", "999").unwrap();
        host.answer_next_prompt(Some(&["0"]));
        host.on_fetch(|_req| {
            Ok(HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: r#"{"data":{"counters":{"endpointCreated":1,"endpointUpdated":0,"endpointFailed":0,"endpointIgnored":0},"errors":[]}}"#.as_bytes().to_vec(),
            })
        });
        Main
            .run_command("export-apifox", &host.invocation(), None)
            .expect("should succeed")
            .expect("summary");
        let requests = host.requests();
        assert_eq!(requests[0].url, "https://api.apifox.com/v1/projects/999/import-openapi?locale=zh-CN");

        // Dismissing the endpoint picker must not send anything.
        let host = project_host();
        host.answer_next_prompt(None);
        let result = Main
            .run_command("export-apifox", &host.invocation(), None)
            .expect("dismiss is not an error");
        assert!(result.is_none());
        assert!(host.requests().is_empty());
    }

    #[test]
    fn export_apifox_reports_apifox_errors_as_warning() {
        let host = project_host();
        host.set_secret(APIFOX_SECRET, "tok");
        host::storage::set("apifox.project_id:/project", "1").unwrap();
        host.answer_next_prompt(Some(&["0"]));
        host.on_fetch(|_req| {
            Ok(HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: r#"{"data":{"counters":{"endpointCreated":0,"endpointUpdated":0,"endpointFailed":1,"endpointIgnored":0},"errors":[{"message":"路径无效","code":"422"}]}}"#.as_bytes().to_vec(),
            })
        });
        let msg = Main
            .run_command("export-apifox", &host.invocation(), None)
            .expect("partial failure is still Ok")
            .expect("summary");
        assert!(msg.contains("失败 1 个"), "unexpected message: {msg}");
        assert!(msg.contains("路径无效"), "unexpected message: {msg}");
    }

    #[test]
    fn configure_apifox_updates_project_id_and_token() {
        let host = project_host();
        host::storage::set("apifox.project_id:/project", "111").unwrap();
        host.set_secret(APIFOX_SECRET, "old-token");
        host.answer_next_prompt(Some(&["222"])); // project id
        host.answer_next_prompt(Some(&["yes"])); // replace token?
        host.answer_next_prompt(Some(&["tok-new"])); // token

        let msg = Main
            .run_command("configure-apifox", &host.invocation(), None)
            .expect("configure should succeed")
            .expect("should report what changed");
        assert!(msg.contains("项目 ID 已设为 222"), "unexpected message: {msg}");
        assert!(msg.contains("访问令牌已更新"), "unexpected message: {msg}");
        assert_eq!(
            host.storage().get("apifox.project_id:/project").map(String::as_str),
            Some("222")
        );
    }

    #[test]
    fn configure_apifox_can_clear_saved_project_id() {
        let host = project_host();
        host::storage::set("apifox.project_id:/project", "111").unwrap();
        host.set_secret(APIFOX_SECRET, "keep-me");
        host.answer_next_prompt(Some(&[""])); // clear the id
        host.answer_next_prompt(None); // do not touch the token

        let msg = Main
            .run_command("configure-apifox", &host.invocation(), None)
            .expect("configure should succeed")
            .expect("should report what changed");
        assert!(msg.contains("已删除项目 ID"), "unexpected message: {msg}");
        assert!(!host.storage().contains_key("apifox.project_id:/project"));
        assert!(host::secret_is_set(APIFOX_SECRET), "token must be left alone");
    }

    #[test]
    fn configure_apifox_can_delete_the_token() {
        let host = project_host();
        host::storage::set("apifox.project_id:/project", "111").unwrap();
        host.set_secret(APIFOX_SECRET, "old-token");
        host.answer_next_prompt(Some(&["111"])); // unchanged id
        host.answer_next_prompt(Some(&["yes"]));
        host.answer_next_prompt(Some(&[""])); // empty token = delete

        let msg = Main
            .run_command("configure-apifox", &host.invocation(), None)
            .expect("configure should succeed")
            .expect("should report what changed");
        assert!(msg.contains("已删除访问令牌"), "unexpected message: {msg}");
        assert!(!host::secret_is_set(APIFOX_SECRET), "token should be gone");
    }

    #[test]
    fn manifest_declares_the_project_view_menu() {
        let toml = include_str!("../plugin.toml");
        assert!(toml.contains("menu = \"projectView.context\""));
        assert!(toml.contains("command = \"export-apifox\""));
        assert!(toml.contains("net:api.apifox.com"));
        assert!(toml.contains("name = \"apifox_token\""));
        // All four actions live under one submenu.
        assert_eq!(toml.matches("submenu = \"Spring Api Export\"").count(), 4);
        for command in ["export-apifox", "export-markdown", "export-openapi", "configure-apifox"] {
            assert!(
                toml.contains(&format!("command = \"{command}\"\nsubmenu")),
                "menu entry for {command} should be in the submenu"
            );
        }
        // Identity: id / name / author / icon.
        assert!(toml.contains("id = \"yuyuy.spring-api-export\""));
        assert!(toml.contains("name = \"Spring Api Export\""));
        assert!(toml.contains("authors = [\"yuyuy\"]"));
        assert!(toml.contains("icon = \"icon.svg\""));
    }

    /// Reads a file the plugin wrote through the public project API.
    fn written(rel: &str) -> String {
        let abs = format!("/project/{rel}");
        let project = host::project_of(&abs).expect("file should belong to the mock project");
        project.read_file(rel).expect("written file should be readable")
    }
}
