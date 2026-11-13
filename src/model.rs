// Internal model shared by the parser and the exporters.

/// One REST endpoint found in a Spring controller.
#[derive(Debug, Clone, Default)]
pub struct Endpoint {
    /// HTTP method: GET, POST, PUT, DELETE, PATCH.
    pub method: String,
    /// Full path, class mapping + method mapping, `{placeholders}` kept.
    pub path: String,
    /// Controller class simple name.
    pub controller: String,
    /// First line of the controller class Javadoc ("" when absent).
    pub controller_desc: String,
    /// Java method name.
    pub handler: String,
    /// First line of the method Javadoc, or @ApiOperation value.
    pub summary: String,
    /// Rest of the method Javadoc description.
    pub description: String,
    /// Query / path / header parameters.
    pub params: Vec<Param>,
    /// Name of the @RequestBody DTO, if any.
    pub request_body: Option<String>,
    /// Description of the request body (from @param of that name).
    pub request_desc: String,
    /// Payload type after stripping the result envelope, e.g. `UserVo`,
    /// `List<UserVo>` or `Boolean`. `None` means "no data" (void / bare `R`).
    pub response: Option<String>,
    /// Result envelope simple name when the method wraps its payload in one
    /// (`R`, `Result`, `AjaxResult`, ...).
    pub response_envelope: Option<String>,
    /// Description from @return.
    pub return_desc: String,
}

#[derive(Debug, Clone, Default)]
pub struct Param {
    pub name: String,
    /// "query" | "path" | "header"
    pub location: String,
    pub java_type: String,
    pub required: bool,
    pub default: Option<String>,
    pub desc: String,
}

/// A DTO schema (request body or response), with per-field comments.
#[derive(Debug, Clone, Default)]
pub struct Schema {
    /// Simple class name.
    pub name: String,
    /// Class Javadoc first line.
    pub desc: String,
    pub fields: Vec<FieldDoc>,
}

#[derive(Debug, Clone, Default)]
pub struct FieldDoc {
    pub name: String,
    /// Raw Java type as written (e.g. `List<UserVo>`).
    pub java_type: String,
    pub desc: String,
    pub required: bool,
}
