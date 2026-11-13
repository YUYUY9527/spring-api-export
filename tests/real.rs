// Temporary harness: run the full extraction pipeline on real Spring files and
// print the resulting endpoints + OpenAPI JSON, to verify params/responses.
use openidea_plugin_yuyuy_spring_api_export::java::JavaFile;
use openidea_plugin_yuyuy_spring_api_export::markdown::DtoResolver;
use openidea_plugin_yuyuy_spring_api_export::openapi::OpenApiWriter;
use openidea_plugin_yuyuy_spring_api_export::spring::{expand_object_params, is_controller, parse_endpoints};

#[test]
fn dump_real_files() {
    let Ok(list) = std::env::var("REAL_FILES") else {
        eprintln!("REAL_FILES not set, skipping");
        return;
    };
    let mut endpoints = Vec::new();
    let mut resolver = DtoResolver::new();
    for path in list.split(';').filter(|s| !s.trim().is_empty()) {
        let src = std::fs::read_to_string(path).expect("read file");
        let f = JavaFile::parse(&src);
        println!("\n===== {} classes={}", path, f.classes.len());
        for c in &f.classes {
            println!("class {} kind={} ctrl={} fields={} methods={}", c.name, c.kind, is_controller(c), c.fields.len(), c.methods.len());
            if is_controller(c) {
                for m in &c.methods {
                    endpoints.extend(parse_endpoints(c, m));
                }
            }
            resolver.add_class(c.clone());
        }
    }
    expand_object_params(&mut endpoints, &resolver.lookup);
    let refs = resolver.resolve(&endpoints);
    for e in &endpoints {
        println!(
            "\n{} {}  summary={:?}\n  params={:?}\n  body={:?} desc={:?}\n  envelope={:?} payload={:?}",
            e.method,
            e.path,
            e.summary,
            e.params.iter().map(|p| format!("{}@{}:{}", p.name, p.location, p.java_type)).collect::<Vec<_>>(),
            e.request_body,
            e.request_desc,
            e.response_envelope,
            e.response,
        );
    }
    println!("\n===== resolved schemas: {:?}", refs.schemas.keys().collect::<Vec<_>>());
    println!("\n===== MARKDOWN =====\n{}", openidea_plugin_yuyuy_spring_api_export::markdown::render(&endpoints, &refs));
    println!("\n===== OPENAPI =====\n{}", OpenApiWriter { title: "t".into() }.render(&endpoints, &refs));
}
