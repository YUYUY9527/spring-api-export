# Spring Api Export

An [OpenIDEA](https://github.com/l0ng-ai/openidea) plugin that reads Spring Boot
controllers from the open project and turns them into API documentation —
Markdown or OpenAPI 3.0 JSON — then pushes selected endpoints into an
[Apifox](https://apifox.com) project with one request.

It parses the Java source directly (no build needed, no Spring context started):
`@RestController` / `@Controller` classes, `@GetMapping` / `@PostMapping` / …,
`@RequestParam` / `@PathVariable` / `@RequestHeader` / `@RequestBody`, Javadoc
(summary, `@param`, `@return`), DTO field comments (`/** */`, `@Schema`,
`@ApiModelProperty`) and required-ness (`@NotNull` and friends, primitive
types). Response envelopes such as `R<T>`, `Result<T>`, `AjaxResult<T>` are
kept as the outer object with the payload inside; `IPage<T>` / `Page<T>` become
a page object with a `records` array.

## Usage

Right-click a controller file or a folder in the Project view and pick
**Spring Api Export ▸**:

| Menu entry | What it does |
| --- | --- |
| Export to Apifox | Scans, shows the endpoint list (with a select-all entry), imports the picked ones into your Apifox project |
| Export Markdown API Doc | Writes `docs/api.md` |
| Export OpenAPI JSON | Writes `docs/openapi.json` |
| Configure Apifox (project id / token) | Changes the Apifox project id, or resets / clears the access token |

The same commands are in **Find Action** (`Spring Api Export: …`); without a
project-view selection they scan the whole project. Right-clicking a file keeps
the export to that file (or folder).

## Configuration

- **Access token** — one token for everything, stored in the OS keychain and
  never readable by the plugin code. Enter it in the first Apifox export, or in
  Settings › Plugins › Spring Api Export. Create it in Apifox › Account
  Settings › Open API. Requests carry `{{secret:apifox_token}}`; the host fills
  it in on the way out.
- **Project id** — remembered per IDE project (each project keeps its own), set
  on the first export or later via *Configure Apifox*. It is the number in the
  Apifox project URL / project settings.

Existing endpoints with the same method and path are merged
(`AUTO_MERGE`), so hand edits made in Apifox survive a re-export.

## Capabilities, and why

| Capability | Why it is needed |
| --- | --- |
| `project.read` | Read the project's `.java` sources to parse controllers and DTOs. Read-only. |
| `project.write` | Write the documentation files (`docs/api.md`, `docs/openapi.json`) into the project. Used only by the two export commands. |
| `storage` | Remember the Apifox project id per IDE project, so you are not asked again. |
| `secrets` | Keep the Apifox access token in the system keychain. The plugin only checks whether it is set; it never reads the value — the host injects it into the outgoing request. |
| `net:api.apifox.com` | Send the generated OpenAPI document to Apifox's import API (`POST https://api.apifox.com/v1/projects/{id}/import-openapi`). Only happens when you run *Export to Apifox*; no other network access, no telemetry. |

## Limitations

- Java sources only (no Kotlin), and no classpath resolution: types whose source
  is not in the project are shown as an object placeholder (with a note), and
  generic envelopes without source fall back to the conventional
  `code` / `msg` / `data` shape marked as inferred.
- Page structures are inferred from the mybatis-plus convention
  (`records` / `total` / `size` / `current` / `pages`).
- Apifox import does not use folders or tags to group endpoints; everything goes
  into the target project's default location.

## Build

```sh
cargo install --locked --git https://github.com/l0ng-ai/openidea ide-host --bin openidea-plugin
openidea-plugin build          # -> dist/plugin.wasm
openidea-plugin link           # load this folder into a running OpenIDEA
cargo test                     # unit tests against the SDK's mock host
```

Requires plugin API 1.4+ (`engines.openidea = "^1.4"`), i.e. the project-write,
storage, secrets and `net:` capabilities.

## License

MIT — see [LICENSE](LICENSE).
