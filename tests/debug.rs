use openidea_plugin_yuyuy_spring_api_export::java::JavaFile;
use openidea_plugin_yuyuy_spring_api_export::spring::{is_controller, parse_endpoint};

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

#[test]
fn debug_controller() {
    let f = JavaFile::parse(CONTROLLER);
    for c in &f.classes {
        println!("class {} kind={} ctrl={} methods={}", c.name, c.kind, is_controller(c), c.methods.len());
        for m in &c.methods {
            println!(
                "  method {} ret={:?} params={:?} anns={:?} javadoc={:?}",
                m.name,
                m.return_type,
                m.params_text,
                m.anns.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
                m.javadoc.as_ref().map(|j| &j.desc)
            );
            println!("    -> endpoint {:?}", parse_endpoint(c, m).map(|e| (e.method, e.path, e.summary)));
        }
    }
}
