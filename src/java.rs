// Minimal Java source scanner: comment/string-aware, char-offset based.
//
// All internal offsets are *char* positions (not bytes) so Chinese comments
// never shift anything. Two views of one file:
//   code : comments blanked to spaces, strings intact  (extract string literals)
//   scan : comments AND string/char literal contents blanked (structural parsing)

#[derive(Debug, Clone)]
pub struct Comment {
    pub start: usize, // char offset of '/'
    pub end: usize,   // char offset just past the comment
    pub text: String, // raw text from the original source
    pub javadoc: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Javadoc {
    pub desc: String,
    /// @param name -> description
    pub params: Vec<(String, String)>,
    pub ret: String,
}

#[derive(Debug, Clone)]
pub struct Ann {
    pub name: String,
    /// Argument text between the parens (strings intact, comments gone). Empty when none.
    pub args: String,
}

#[derive(Debug, Clone)]
pub struct FieldRaw {
    pub name: String,
    pub java_type: String,
    pub anns: Vec<Ann>,
    pub javadoc: Option<Javadoc>,
    /// `static` fields are constants (e.g. serialVersionUID) and never part of a DTO payload.
    pub is_static: bool,
}

#[derive(Debug, Clone)]
pub struct MethodRaw {
    pub name: String,
    pub return_type: String,
    /// Full parameter list text (strings intact, comments gone).
    pub params_text: String,
    pub anns: Vec<Ann>,
    pub javadoc: Option<Javadoc>,
}

#[derive(Debug, Clone)]
pub struct JavaClass {
    pub name: String,
    /// "class" | "interface" | "enum" | "record" (diagnostics; not yet used by exporters)
    #[allow(dead_code)]
    pub kind: String,
    pub anns: Vec<Ann>,
    pub javadoc: Option<Javadoc>,
    pub fields: Vec<FieldRaw>,
    pub methods: Vec<MethodRaw>,
}

#[derive(Debug, Clone, Default)]
pub struct JavaFile {
    pub code: Vec<char>,         // comments blanked
    pub scan: Vec<char>,         // comments + string contents blanked
    pub comments: Vec<Comment>,
    pub classes: Vec<JavaClass>, // top-level and nested classes
}

impl JavaFile {
    pub fn parse(src: &str) -> JavaFile {
        let chars: Vec<char> = src.chars().collect();
        let mut f = JavaFile::default();

        // char idx -> byte offset, for slicing the original text
        let mut byte_of: Vec<usize> = Vec::with_capacity(chars.len() + 1);
        {
            let mut b = 0usize;
            for c in &chars {
                byte_of.push(b);
                b += c.len_utf8();
            }
            byte_of.push(b);
        }

        // ---- pass 1: comment/string scan, build `code` ----
        let mut code: Vec<char> = Vec::with_capacity(chars.len());
        let n = chars.len();
        let mut i = 0usize;
        let mut state = 0u8; // 0 normal, 3 string, 4 char, 5 text block
        while i < n {
            let c = chars[i];
            match state {
                0 => {
                    if c == '/' && i + 1 < n && chars[i + 1] == '/' {
                        let start = i;
                        while i < n && chars[i] != '\n' {
                            code.push(' ');
                            i += 1;
                        }
                        f.comments.push(Comment { start, end: i, text: self_slice(&chars, &byte_of, start, i), javadoc: false });
                    } else if c == '/' && i + 1 < n && chars[i + 1] == '*' {
                        let start = i;
                        let jd = i + 2 < n && chars[i + 2] == '*';
                        code.push(' ');
                        code.push(' ');
                        i += 2;
                        while i < n {
                            if chars[i] == '*' && i + 1 < n && chars[i + 1] == '/' {
                                code.push(' ');
                                code.push(' ');
                                i += 2;
                                break;
                            }
                            code.push(if chars[i] == '\n' { '\n' } else { ' ' });
                            i += 1;
                        }
                        f.comments.push(Comment { start, end: i, text: self_slice(&chars, &byte_of, start, i), javadoc: jd });
                    } else if c == '"' && i + 2 < n && chars[i + 1] == '"' && chars[i + 2] == '"' {
                        code.push('"'); code.push('"'); code.push('"');
                        i += 3;
                        state = 5;
                    } else if c == '"' {
                        code.push('"');
                        i += 1;
                        state = 3;
                    } else if c == '\'' {
                        code.push('\'');
                        i += 1;
                        state = 4;
                    } else {
                        code.push(c);
                        i += 1;
                    }
                }
                3 => {
                    if c == '\\' && i + 1 < n {
                        code.push(c); code.push(chars[i + 1]); i += 2;
                    } else if c == '"' {
                        code.push('"'); i += 1; state = 0;
                    } else {
                        code.push(c); i += 1;
                    }
                }
                4 => {
                    if c == '\\' && i + 1 < n {
                        code.push(c); code.push(chars[i + 1]); i += 2;
                    } else if c == '\'' {
                        code.push('\''); i += 1; state = 0;
                    } else {
                        code.push(c); i += 1;
                    }
                }
                _ => {
                    // text block
                    if c == '"' && i + 2 < n && chars[i + 1] == '"' && chars[i + 2] == '"' {
                        code.push('"'); code.push('"'); code.push('"');
                        i += 3; state = 0;
                    } else if c == '\\' && i + 1 < n {
                        code.push(c); code.push(chars[i + 1]); i += 2;
                    } else {
                        code.push(c); i += 1;
                    }
                }
            }
        }

        // ---- pass 2: blank string/char contents for `scan` ----
        let mut scan: Vec<char> = Vec::with_capacity(code.len());
        {
            let mut i = 0usize;
            let mut state = 0u8;
            while i < code.len() {
                let c = code[i];
                match state {
                    0 => {
                        if c == '"' && i + 2 < code.len() && code[i + 1] == '"' && code[i + 2] == '"' {
                            scan.push('"'); scan.push('"'); scan.push('"');
                            i += 3; state = 5;
                        } else if c == '"' {
                            scan.push('"'); i += 1; state = 3;
                        } else if c == '\'' {
                            scan.push('\''); i += 1; state = 4;
                        } else {
                            scan.push(c); i += 1;
                        }
                    }
                    3 | 4 => {
                        if c == '\\' && i + 1 < code.len() {
                            scan.push(' '); scan.push(' '); i += 2;
                        } else if c == '"' || c == '\'' {
                            scan.push(c); i += 1; state = 0;
                        } else {
                            scan.push(' '); i += 1;
                        }
                    }
                    _ => {
                        if c == '"' && i + 2 < code.len() && code[i + 1] == '"' && code[i + 2] == '"' {
                            scan.push('"'); scan.push('"'); scan.push('"');
                            i += 3; state = 0;
                        } else if c == '\\' && i + 1 < code.len() {
                            scan.push(' '); scan.push(' '); i += 2;
                        } else {
                            scan.push(' '); i += 1;
                        }
                    }
                }
            }
        }

        f.code = code;
        f.scan = scan;
        f.parse_classes();
        f
    }

    fn parse_classes(&mut self) {
        let s = self.scan.clone();
        let n = s.len();
        let mut i = 0usize;
        while i < n {
            if starts_word(&s, i, "class")
                || starts_word(&s, i, "interface")
                || starts_word(&s, i, "enum")
                || starts_word(&s, i, "record")
            {
                // must follow whitespace / block boundary — excludes `Foo.class`
                let ok_left = i == 0
                    || s[i - 1].is_whitespace()
                    || matches!(s[i - 1], '{' | '}' | ';' | ')' | ',');
                if ok_left {
                    if let Some(end) = self.parse_one_class(i, &s) {
                        i = end;
                        continue;
                    }
                }
            }
            i += 1;
        }
    }

    /// Parses the class whose `class`/`interface`/`enum`/`record` keyword starts at `kw`.
    /// `s` is the current scan view. Returns the char offset just past the class body.
    fn parse_one_class(&mut self, kw: usize, s: &[char]) -> Option<usize> {
        let code = self.code.clone();
        let n = s.len();
        let kind = {
            if starts_word(s, kw, "interface") { "interface" }
            else if starts_word(s, kw, "enum") { "enum" }
            else if starts_word(s, kw, "record") { "record" }
            else { "class" }
        };

        // find first '{' outside parens (records have a header param list)
        let mut i = kw;
        let mut paren = 0i32;
        while i < n {
            match s[i] {
                '(' => paren += 1,
                ')' => paren -= 1,
                '{' if paren == 0 => break,
                ';' if paren == 0 => return Some(i), // e.g. `class A;` weirdness
                _ => {}
            }
            i += 1;
        }
        if i >= n {
            return Some(i);
        }
        let body_open = i;
        let body_close = match_forward(s, body_open, '{', '}')?;

        // Class name = first identifier right after the keyword. Never scan
        // backwards: `class Foo extends Model<Foo>` would yield `Model`.
        let name = {
            let mut k = kw;
            while k < body_open && is_id(s[k]) {
                k += 1; // skip the keyword itself
            }
            while k < body_open && !is_id(s[k]) {
                k += 1; // skip whitespace
            }
            let start = k;
            while k < body_open && is_id(s[k]) {
                k += 1;
            }
            s[start..k].iter().collect::<String>()
        };
        if name.is_empty() {
            return Some(body_close + 1);
        }

        let anns = collect_annotations(s, &code, kw);
        let javadoc = self.javadoc_before(kw);

        let mut cls = JavaClass {
            name,
            kind: kind.to_string(),
            anns,
            javadoc,
            fields: Vec::new(),
            methods: Vec::new(),
        };

        // Walk the class body.
        let mut j = body_open + 1;
        let mut stmt_start = j;
        let mut depth = 0i32;
        let mut paren = 0i32;
        while j < body_close {
            let c = s[j];
            match c {
                '{' => {
                    if depth == 0 && paren == 0 {
                        let header: String = s[stmt_start..j].iter().collect();
                        let header_t = header.trim();
                        let is_type = ["class ", "interface ", "enum ", "record "]
                            .iter()
                            .any(|w| find_word_in(header_t, w.trim_end()));
                        let has_paren = header_t.contains('(');
                        // An '=' before the first '(' marks a field initializer or
                        // lambda (`int[] a = {...}`, `Runnable r = () -> {...}`);
                        // an '=' after it is an annotation argument
                        // (`@RequestParam(defaultValue = "1")`) and the header is
                        // still a method.
                        let init_before_paren = match (header_t.find('='), header_t.find('(')) {
                            (Some(eq), Some(paren)) => eq < paren,
                            (Some(_), None) => true,
                            _ => false,
                        };
                        if is_type && !has_paren {
                            // nested type (annotations may precede)
                            if let Some(kw_off) = self.find_type_keyword(stmt_start, j) {
                                if let Some(end) = self.parse_one_class(kw_off, s) {
                                    j = end;
                                    stmt_start = j;
                                    continue;
                                }
                            }
                        } else if has_paren && !init_before_paren {
                            // method
                            if let Some(m) = self.parse_method(stmt_start, j, s, &code) {
                                cls.methods.push(m);
                            }
                            let close = match_forward(s, j, '{', '}')?;
                            j = close + 1;
                            stmt_start = j;
                            continue;
                        } else {
                            // initializer block / array literal / lambda field — skip the block
                            let close = match_forward(s, j, '{', '}')?;
                            j = close + 1;
                            stmt_start = j;
                            continue;
                        }
                    }
                    depth += 1;
                    j += 1;
                }
                '}' => {
                    depth -= 1;
                    j += 1;
                }
                ';' => {
                    if depth == 0 && paren == 0 {
                        self.parse_field(stmt_start, j, s, &code, &mut cls);
                        j += 1;
                        stmt_start = j;
                        continue;
                    }
                    j += 1;
                }
                '(' => { paren += 1; j += 1; }
                ')' => { paren -= 1; j += 1; }
                _ => j += 1,
            }
        }
        self.classes.push(cls);
        Some(body_close + 1)
    }

    fn find_type_keyword(&self, stmt_start: usize, brace: usize) -> Option<usize> {
        let s = &self.scan;
        for i in stmt_start..brace {
            for w in ["class", "interface", "enum", "record"] {
                if starts_word(s, i, w)
                    && (i == 0 || s[i - 1].is_whitespace() || matches!(s[i - 1], '{' | '}' | ';' | ')' | ','))
                {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Parses a field statement ending at `semi` (exclusive).
    fn parse_field(&self, stmt_start: usize, semi: usize, s: &[char], code: &[char], cls: &mut JavaClass) {
        let header = strip_annotations(&s[stmt_start..semi].iter().collect::<String>());
        let t = header.trim();
        if t.is_empty() || t.contains('(') {
            return;
        }
        // take text before '=' if present
        let decl = match t.find('=') {
            Some(p) => t[..p].trim_end(),
            None => t,
        };
        let name = match decl.rsplit(|c: char| !is_id_c(c)).next() {
            Some(w) if !w.is_empty() && w.chars().all(is_id_c) => w.to_string(),
            _ => return,
        };
        if is_keyword(&name) {
            return;
        }
        let before = decl[..decl.len() - name.len()].trim();
        // type must start with an identifier char (rejects garbage from skipped blocks)
        if before.is_empty() || !is_id_c(before.chars().next().unwrap()) {
            return;
        }
        let ty = before
            .split_whitespace()
            .filter(|w| {
                !matches!(*w, "public" | "private" | "protected" | "static" | "final" | "transient" | "volatile" | "abstract")
            })
            .collect::<Vec<_>>()
            .join(" ");
        if ty.is_empty() {
            return;
        }
        let anns = collect_annotations_range(s, code, stmt_start, semi);
        let javadoc = self.javadoc_in_span(stmt_start, semi);
        let is_static = decl[..decl.len() - name.len()]
            .split_whitespace()
            .any(|w| w == "static");
        cls.fields.push(FieldRaw { name, java_type: ty, anns, javadoc, is_static });
    }

    /// Parses a method whose header spans [header_start, brace) — `brace` is the body '{'.
    fn parse_method(&self, header_start: usize, brace: usize, s: &[char], code: &[char]) -> Option<MethodRaw> {
        // locate the real parameter '(' skipping annotation blocks like @Foo("x")
        let mut open_off = None;
        let mut i = header_start;
        while i < brace {
            if s[i] == '@' {
                i += 1;
                while i < brace && is_id(s[i]) {
                    i += 1;
                }
                while i < brace && s[i].is_whitespace() {
                    i += 1;
                }
                if i < brace && s[i] == '(' {
                    i = match_forward(s, i, '(', ')')? + 1;
                }
                continue;
            }
            if s[i] == '(' {
                open_off = Some(i);
                break;
            }
            i += 1;
        }
        let open_off = open_off?;
        let close_off = match_forward(s, open_off, '(', ')')?;
        let params_text: String = code[open_off + 1..close_off].iter().collect();

        // name = identifier right before the '('
        let mut j = open_off;
        while j > header_start && s[j - 1].is_whitespace() {
            j -= 1;
        }
        let mut k = j;
        while k > header_start && is_id(s[k - 1]) {
            k -= 1;
        }
        let name: String = s[k..j].iter().collect();
        if name.is_empty() || is_keyword(&name) {
            return None;
        }

        // return type = tokens between header_start and the name, minus
        // annotation blocks (their args are blanked in the scan view) and modifiers
        let mut ret_toks: Vec<String> = Vec::new();
        let mut i = header_start;
        while i < k {
            while i < k && s[i].is_whitespace() {
                i += 1;
            }
            if i >= k {
                break;
            }
            if s[i] == '@' {
                i += 1;
                while i < k && is_id(s[i]) {
                    i += 1;
                }
                while i < k && s[i].is_whitespace() {
                    i += 1;
                }
                if i < k && s[i] == '(' {
                    i = match_forward(s, i, '(', ')')? + 1;
                }
                continue;
            }
            let start = i;
            while i < k && !s[i].is_whitespace() {
                i += 1;
            }
            ret_toks.push(s[start..i].iter().collect());
        }
        let ret = ret_toks
            .into_iter()
            .filter(|w| {
                !matches!(
                    w.as_str(),
                    "public" | "private" | "protected" | "static" | "final" | "abstract"
                        | "synchronized" | "native" | "default" | "strictfp"
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        if ret.is_empty() {
            return None; // constructor
        }

        let anns = collect_annotations_range(s, code, header_start, brace);
        let javadoc = self.javadoc_in_span(header_start, brace);
        Some(MethodRaw { name, return_type: ret, params_text, anns, javadoc })
    }

    /// Last javadoc comment before `pos` such that only whitespace/annotations sit between.
    fn javadoc_before(&self, pos: usize) -> Option<Javadoc> {
        self.javadoc_before_range(pos)
    }

    fn javadoc_before_range(&self, from: usize) -> Option<Javadoc> {
        let s = &self.scan;
        let mut best: Option<&Comment> = None;
        for c in &self.comments {
            if !c.javadoc || c.end > from {
                continue;
            }
            let between = &s[c.end..from];
            // `"` and `=` appear when annotations with (blanked) string args sit between
            if between.iter().all(|ch| {
                ch.is_whitespace() || *ch == '@' || *ch == '(' || *ch == ')' || *ch == '"' || *ch == '=' || is_id(*ch)
            }) {
                best = Some(c);
            }
        }
        best.filter(|c| from - c.end < 400).map(|c| parse_javadoc(&c.text))
    }

    /// Javadoc for a member whose declaration spans [a, b): the comment sits
    /// INSIDE the span (after stmt_start), followed by only annotations and
    /// the member signature up to b.
    fn javadoc_in_span(&self, a: usize, b: usize) -> Option<Javadoc> {
        let s = &self.scan;
        let mut best: Option<&Comment> = None;
        for c in &self.comments {
            if !c.javadoc || c.start < a || c.end > b {
                continue;
            }
            let between = &s[c.end..b];
            if between.iter().all(|ch| {
                ch.is_whitespace()
                    || matches!(*ch, '@' | '(' | ')' | '"' | '=' | ',' | ':' | '<' | '>')
                    || is_id(*ch)
            }) {
                best = Some(c);
            }
        }
        best.filter(|c| b - c.end < 600).map(|c| parse_javadoc(&c.text))
    }
}

// ---------------------------------------------------------------------------
// free helpers
// ---------------------------------------------------------------------------

fn self_slice(chars: &[char], _byte_of: &[usize], a: usize, z: usize) -> String {
    chars[a..z].iter().collect()
}

pub fn is_id(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// Remove annotation tokens `@Name` / `@Name(...)` (balanced) from a declaration header.
pub fn strip_annotations(t: &str) -> String {
    let c: Vec<char> = t.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '@' {
            i += 1;
            while i < c.len() && is_id(c[i]) {
                i += 1;
            }
            while i < c.len() && c[i].is_whitespace() {
                i += 1;
            }
            if i < c.len() && c[i] == '(' {
                let mut depth = 0i32;
                while i < c.len() {
                    if c[i] == '(' {
                        depth += 1;
                    } else if c[i] == ')' {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    i += 1;
                }
            }
            out.push(' ');
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}
pub fn is_id_c(c: char) -> bool {
    is_id(c)
}

pub fn starts_word(s: &[char], i: usize, w: &str) -> bool {
    let wc: Vec<char> = w.chars().collect();
    if i + wc.len() > s.len() {
        return false;
    }
    s[i..i + wc.len()] == wc[..] && (i + wc.len() >= s.len() || !is_id(s[i + wc.len()]))
}

fn find_word_in(text: &str, w: &str) -> bool {
    let wc: Vec<char> = w.chars().collect();
    let tc: Vec<char> = text.chars().collect();
    if tc.len() < wc.len() {
        return false;
    }
    for i in 0..=tc.len() - wc.len() {
        if tc[i..i + wc.len()] == wc[..]
            && (i + wc.len() >= tc.len() || !is_id(tc[i + wc.len()]))
            && (i == 0 || !is_id(tc[i - 1]))
        {
            return true;
        }
    }
    false
}

/// Index of the matching close for the opener at `at` (1 level).
pub fn match_forward(s: &[char], at: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = at;
    while i < s.len() {
        if s[i] == open {
            depth += 1;
        } else if s[i] == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Annotations immediately preceding `pos` (scanning the whole prefix).
pub fn collect_annotations(s: &[char], code: &[char], pos: usize) -> Vec<Ann> {
    collect_annotations_range(s, code, 0, pos)
}

/// All `@Name(...)`/`@Name` annotations contained in [a, b).
pub fn collect_annotations_range(s: &[char], code: &[char], a: usize, b: usize) -> Vec<Ann> {
    let mut anns = Vec::new();
    let mut i = a;
    while i < b {
        if s[i] == '@' && i + 1 < b && is_id(s[i + 1]) {
            let mut j = i + 1;
            while j < b && is_id(s[j]) {
                j += 1;
            }
            let name: String = s[i + 1..j].iter().collect();
            let mut k = j;
            while k < b && s[k].is_whitespace() {
                k += 1;
            }
            let args = if k < b && s[k] == '(' {
                if let Some(close) = match_forward(s, k, '(', ')') {
                    let text: String = code[k + 1..close].iter().collect();
                    i = close + 1;
                    text
                } else {
                    i = j;
                    String::new()
                }
            } else {
                i = j;
                String::new()
            };
            anns.push(Ann { name, args });
        } else {
            i += 1;
        }
    }
    anns
}

pub fn is_keyword(w: &str) -> bool {
    matches!(w, "if" | "else" | "for" | "while" | "switch" | "return" | "new" | "catch" | "do" | "try" | "throw" | "this" | "super")
}

/// Parses a `/** ... */` comment into description / @param / @return.
pub fn parse_javadoc(text: &str) -> Javadoc {
    let mut body = String::new();
    for line in text.lines() {
        let mut l = line.trim();
        if let Some(r) = l.strip_prefix("/**") {
            l = r;
        }
        if let Some(r) = l.strip_suffix("*/") {
            l = r;
        }
        let l = l.trim_start_matches('*').trim();
        body.push_str(l);
        body.push('\n');
    }
    let mut jd = Javadoc::default();
    let mut cur_tag: Option<String> = None;
    for line in body.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix('@') {
            let mut parts = rest.splitn(2, char::is_whitespace);
            let tag = parts.next().unwrap_or("").to_string();
            let rest = parts.next().unwrap_or("").trim().to_string();
            match tag.as_str() {
                "param" => {
                    let mut it = rest.splitn(2, char::is_whitespace);
                    let pname = it.next().unwrap_or("").to_string();
                    let d = it.next().unwrap_or("").to_string();
                    jd.params.push((pname, d));
                    cur_tag = Some("param".into());
                }
                "return" | "returns" => {
                    jd.ret = rest;
                    cur_tag = Some("return".into());
                }
                _ => cur_tag = None,
            }
        } else if !l.is_empty() {
            match cur_tag.as_deref() {
                None => {
                    if !jd.desc.is_empty() {
                        jd.desc.push(' ');
                    }
                    jd.desc.push_str(l);
                }
                Some("param") => {
                    if let Some(last) = jd.params.last_mut() {
                        last.1.push(' ');
                        last.1.push_str(l);
                    }
                }
                Some("return") => {
                    jd.ret.push(' ');
                    jd.ret.push_str(l);
                }
                _ => {}
            }
        }
    }
    jd
}

// ---------------------------------------------------------------------------
// string literal extraction from `code` (strings intact, comments blanked)
// ---------------------------------------------------------------------------

/// First quoted string literal in `text`, unescaped.
pub fn first_string(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '"' {
            let mut out = String::new();
            let mut j = i + 1;
            while j < chars.len() {
                match chars[j] {
                    '\\' => {
                        if j + 1 < chars.len() {
                            out.push(chars[j + 1]);
                            j += 2;
                        } else {
                            j += 1;
                        }
                    }
                    '"' => return Some(out),
                    c => {
                        out.push(c);
                        j += 1;
                    }
                }
            }
            return None;
        }
        i += 1;
    }
    None
}

/// Splits annotation arguments on top-level commas. Commas inside strings,
/// `(...)`, `{...}`, `<...>` and `[...]` do not split.
pub fn split_args(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut paren, mut brace, mut angle, mut bracket) = (0i32, 0i32, 0i32, 0i32);
    let mut in_str = false;
    let mut escaped = false;
    for c in text.chars() {
        if in_str {
            cur.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                cur.push(c);
            }
            '(' => { paren += 1; cur.push(c); }
            ')' => { paren -= 1; cur.push(c); }
            '{' => { brace += 1; cur.push(c); }
            '}' => { brace -= 1; cur.push(c); }
            '<' => { angle += 1; cur.push(c); }
            '>' => { angle -= 1; cur.push(c); }
            '[' => { bracket += 1; cur.push(c); }
            ']' => { bracket -= 1; cur.push(c); }
            ',' if paren == 0 && brace == 0 && angle == 0 && bracket == 0 => {
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

/// Raw source text of `attr = <value>` in an annotation's argument list, cut
/// at the next top-level comma. Quotes and braces are preserved, so enum
/// constants such as `RequestMethod.POST` can be read back verbatim.
pub fn ann_attr_raw(ann: &Ann, attr: &str) -> Option<String> {
    for seg in split_args(&ann.args) {
        if let Some(rest) = seg.strip_prefix(attr) {
            let rest = rest.trim_start();
            if let Some(v) = rest.strip_prefix('=') {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Annotation attribute's string value: `attr = "v"` or `attr = { "a", "b" }`.
pub fn ann_string(ann: &Ann, attr: &str) -> Option<String> {
    let raw = ann_attr_raw(ann, attr)?;
    let raw = raw.trim_start();
    let raw = raw.strip_prefix('{').map(str::trim_start).unwrap_or(raw);
    first_string(raw)
}

/// Path value of a mapping annotation: `path =`/`value =` or the bare first string.
pub fn ann_path(ann: &Ann) -> Option<String> {
    ann_string(ann, "path")
        .or_else(|| ann_string(ann, "value"))
        .or_else(|| first_string(&ann.args))
}
