use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;
use std::thread;
use std::time::Duration;

use encoding_rs::{Encoding, UTF_8};
use jiff::Timestamp;
use regex::{Captures, Regex};
use serde::Serialize;
use serde_json::{Value, json};

use super::{
    EvaluationError,
    manifest::{SourceSpec, load_sources},
    support::{digest, read_json},
};

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 ",
    "(KHTML, like Gecko) Chrome/122.0 Safari/537.36 ",
    "suiko-corpus-fetch/0.1 (github.com/nwiizo/suiko; research/calibration use)",
);
const RATE_LIMIT: Duration = Duration::from_secs(1);

static CHARSET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)charset\s*=\s*["']?([\w.-]+)"#).expect("valid regex"));
static XML_ENCODING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)encoding\s*=\s*["']([\w.-]+)["']"#).expect("valid regex"));
static NOTE_BODY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?s)<div class="note-common-styles__textnote-body"[^>]*>(.*?)</div>\s*</div>\s*</div>"#,
    )
    .expect("valid note body regex")
});
static ZENN_BODY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<div class="znc"[^>]*>(.*?)</div>\s*</div>\s*</div>"#)
        .expect("valid zenn body regex")
});
static ARTICLE_BODY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<article[^>]*>(.*?)</article>").expect("valid article regex")
});
static MAIN_BODY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<main[^>]*>(.*?)</main>").expect("valid main regex"));
static IGNORED_TAGS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [
        r"(?is)<script[^>]*>.*?</script>",
        r"(?is)<style[^>]*>.*?</style>",
        r"(?is)<noscript[^>]*>.*?</noscript>",
    ]
    .map(|pattern| Regex::new(pattern).expect("valid ignored-tag regex"))
});
static HEADINGS: LazyLock<[Regex; 6]> = LazyLock::new(|| {
    [
        r"(?is)<h1[^>]*>(.*?)</h1>",
        r"(?is)<h2[^>]*>(.*?)</h2>",
        r"(?is)<h3[^>]*>(.*?)</h3>",
        r"(?is)<h4[^>]*>(.*?)</h4>",
        r"(?is)<h5[^>]*>(.*?)</h5>",
        r"(?is)<h6[^>]*>(.*?)</h6>",
    ]
    .map(|pattern| Regex::new(pattern).expect("valid heading regex"))
});
static INLINE_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<[^>]+>").expect("valid tag regex"));
static BREAK_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").expect("valid br regex"));
static PARAGRAPH_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)</p>").expect("valid paragraph regex"));
static DECIMAL_ENTITY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&#(\d+);").expect("valid decimal entity regex"));
static HEX_ENTITY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&#x([0-9a-fA-F]+);").expect("valid hex entity regex"));
static PAGE_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[-‐―ー0-9０-９ページPage/\s.]{1,10}$").expect("valid page number regex")
});

struct Fetched {
    payload: String,
    chars: usize,
    method: &'static str,
}

pub fn fetch_corpus(
    sources_path: &Path,
    selected_id: Option<&str>,
    limit: Option<usize>,
) -> Result<String, EvaluationError> {
    let manifest = load_sources(sources_path)?;

    let mut sources = manifest
        .source
        .into_iter()
        .filter(|source| source.kind == "web")
        .collect::<Vec<_>>();
    if let Some(id) = selected_id {
        sources.retain(|source| source.id == id);
        if sources.is_empty() {
            return Err(EvaluationError::Invalid(format!("id not found: {id}")));
        }
    } else if let Some(limit) = limit.filter(|limit| *limit > 0) {
        sources.truncate(limit);
    }

    let base = sources_path.parent().unwrap_or_else(|| Path::new("."));
    let out_dir = base.join("corpus/external");
    let lock_path = base.join("corpus/external-lock.json");
    fs::create_dir_all(&out_dir).map_err(|source| EvaluationError::Write {
        path: out_dir.display().to_string(),
        source,
    })?;
    let mut lock = load_lock(&lock_path)?;
    let entries = lock_entry_map(&mut lock)?;

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent(USER_AGENT)
        .build()
        .into();
    let total = sources.len();
    let mut failed = 0;
    for (index, source) in sources.iter().enumerate() {
        eprintln!("fetch: {} <- {}", source.id, source.url);
        let fetched_at = fetched_at();
        let result = fetch_one(&agent, source).and_then(|fetched| {
            let path = out_dir.join(format!("{}.md", source.id));
            fs::write(&path, fetched.payload.as_bytes())
                .map_err(|error| format!("write {}: {error}", path.display()))?;
            Ok((fetched, path))
        });
        match result {
            Ok((fetched, path)) => {
                let sha256 = digest(fetched.payload.as_bytes());
                entries.insert(
                    source.id.clone(),
                    json!({
                        "url": source.url,
                        "sha256": sha256,
                        "chars": fetched.chars,
                        "extract_method": fetched.method,
                        "fetched_at": fetched_at,
                    }),
                );
                eprintln!(
                    "  -> {} ({}, {} chars)",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    fetched.method,
                    fetched.chars
                );
            }
            Err(error) => {
                failed += 1;
                entries.insert(
                    source.id.clone(),
                    json!({
                        "url": source.url,
                        "error": error,
                        "fetched_at": fetched_at,
                    }),
                );
                eprintln!("  ERROR: {}: {error}", source.id);
            }
        }
        if index + 1 < total {
            thread::sleep(RATE_LIMIT);
        }
    }
    save_lock(&lock_path, &lock)?;

    let succeeded = total - failed;
    let summary = format!(
        "summary: {succeeded} succeeded, {failed} failed (total {total}); lock -> {}",
        lock_path.display()
    );
    if failed > 0 {
        return Err(EvaluationError::Invalid(summary));
    }
    eprintln!("{summary}");
    Ok(String::new())
}

fn fetch_one(agent: &ureq::Agent, source: &SourceSpec) -> Result<Fetched, String> {
    let response = agent
        .get(&source.url)
        .call()
        .map_err(|error| error.to_string())?;
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let mut content = Vec::new();
    response
        .into_body()
        .into_reader()
        .read_to_end(&mut content)
        .map_err(|error| error.to_string())?;

    let is_pdf = content_type
        .to_ascii_lowercase()
        .contains("application/pdf")
        || source
            .url
            .split('?')
            .next()
            .is_some_and(|url| url.to_ascii_lowercase().ends_with(".pdf"));
    let (method, body) = if is_pdf {
        ("pdf", extract_pdf(&content)?)
    } else {
        let html = decode_html(&content_type, &content);
        extract_body(&source.url, &html)
    };
    let chars = body.chars().count();
    // titleとauthorは自由記述なので、frontmatterで安全なquoted scalarにする。
    let title = serde_json::to_string(&source.title).map_err(|error| error.to_string())?;
    let author = serde_json::to_string(&source.author).map_err(|error| error.to_string())?;
    let payload = format!(
        "---\nid: {}\nsource_url: {}\ntitle: {}\nauthor: {}\ngenre: {}\nextract_method: {}\nchars: {}\n---\n\n{}\n",
        source.id,
        source.url,
        title,
        author,
        source.genre.as_str(),
        method,
        chars,
        body
    );
    Ok(Fetched {
        payload,
        chars,
        method,
    })
}

fn decode_html(content_type: &str, content: &[u8]) -> String {
    let header_label = CHARSET
        .captures(content_type)
        .and_then(|captures| captures.get(1));
    let head = String::from_utf8_lossy(&content[..content.len().min(2048)]);
    let embedded_label = CHARSET
        .captures(&head)
        .or_else(|| XML_ENCODING.captures(&head))
        .and_then(|captures| captures.get(1));
    let encoding = header_label
        .or(embedded_label)
        .and_then(|label| Encoding::for_label(label.as_str().as_bytes()))
        .unwrap_or(UTF_8);
    let (decoded, _, _) = encoding.decode(content);
    decoded.into_owned()
}

fn extract_body(url: &str, html: &str) -> (&'static str, String) {
    for (domain, pattern, method) in [
        ("note.com", &*NOTE_BODY, "note"),
        ("zenn.dev", &*ZENN_BODY, "zenn"),
    ] {
        if !url.contains(domain) {
            continue;
        }
        let Some(text) = extract_site_body(html, pattern) else {
            continue;
        };
        if text.chars().count() > 200 {
            return (method, text);
        }
    }
    ("generic", extract_generic(html))
}

fn extract_site_body(html: &str, primary: &Regex) -> Option<String> {
    capture_body(html, primary)
        .or_else(|| capture_body(html, &ARTICLE_BODY))
        .map(|body| strip_tags(&body))
}

fn extract_generic(html: &str) -> String {
    let body = capture_body(html, &ARTICLE_BODY)
        .or_else(|| capture_body(html, &MAIN_BODY))
        .unwrap_or_else(|| html.to_owned());
    strip_tags(&body)
}

fn capture_body(html: &str, pattern: &Regex) -> Option<String> {
    pattern
        .captures(html)
        .and_then(|captures| captures.get(1))
        .map(|body| body.as_str().to_owned())
}

fn strip_tags(html: &str) -> String {
    let mut text = html.to_owned();
    for pattern in IGNORED_TAGS.iter() {
        text = pattern.replace_all(&text, "").into_owned();
    }
    for (index, pattern) in HEADINGS.iter().enumerate() {
        let level = index + 1;
        text = pattern
            .replace_all(&text, |captures: &Captures<'_>| {
                let inner = INLINE_TAG.replace_all(&captures[1], "");
                let heading = unescape_entities(&inner)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if heading.is_empty() {
                    String::new()
                } else {
                    format!("\n\n{} {}\n\n", "#".repeat(level), heading)
                }
            })
            .into_owned();
    }
    text = BREAK_TAG.replace_all(&text, "\n").into_owned();
    text = PARAGRAPH_END.replace_all(&text, "\n\n").into_owned();
    text = INLINE_TAG.replace_all(&text, "").into_owned();
    unescape_entities(&text)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn unescape_entities(text: &str) -> String {
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"");
    let text = DECIMAL_ENTITY
        .replace_all(&text, |captures: &Captures<'_>| {
            captures[1]
                .parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .map(|value| value.to_string())
                .unwrap_or_else(|| captures[0].to_owned())
        })
        .into_owned();
    HEX_ENTITY
        .replace_all(&text, |captures: &Captures<'_>| {
            u32::from_str_radix(&captures[1], 16)
                .ok()
                .and_then(char::from_u32)
                .map(|value| value.to_string())
                .unwrap_or_else(|| captures[0].to_owned())
        })
        .into_owned()
}

fn extract_pdf(content: &[u8]) -> Result<String, String> {
    let pages =
        pdf_extract::extract_text_from_mem_by_pages(content).map_err(|error| error.to_string())?;
    Ok(clean_pdf_pages(&pages))
}

fn clean_pdf_pages(pages: &[String]) -> String {
    let mut counts = BTreeMap::<String, usize>::new();
    for page in pages {
        let unique = page
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && line.chars().count() <= 40)
            .collect::<BTreeSet<_>>();
        for line in unique {
            *counts.entry(line.to_owned()).or_default() += 1;
        }
    }
    let repeated = counts
        .into_iter()
        .filter(|(_, count)| pages.len() > 2 && *count >= 3.max(pages.len() / 2))
        .map(|(line, _)| line)
        .collect::<BTreeSet<_>>();
    pages
        .iter()
        .flat_map(|page| page.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty() && !repeated.contains(*line) && !PAGE_NUMBER.is_match(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn fetched_at() -> String {
    Timestamp::now().strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn load_lock(path: &Path) -> Result<Value, EvaluationError> {
    if !path.exists() {
        return Ok(json!({"version": 1, "entries": {}}));
    }
    read_json(path)
}

fn lock_entry_map(
    lock: &mut Value,
) -> Result<&mut serde_json::Map<String, Value>, EvaluationError> {
    lock.get_mut("entries")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            EvaluationError::Invalid(
                "external-lock.json の entries はobjectである必要があります".to_owned(),
            )
        })
}

fn save_lock(path: &Path, lock: &Value) -> Result<(), EvaluationError> {
    let mut bytes = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
    lock.serialize(&mut serializer)
        .map_err(|error| EvaluationError::Parse {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|source| EvaluationError::Write {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::{clean_pdf_pages, extract_body};

    #[test]
    fn site_specific_extraction_uses_the_named_container() {
        let body = "固有本文です。".repeat(30);
        for (url, class, method) in [
            (
                "https://note.com/example",
                "note-common-styles__textnote-body",
                "note",
            ),
            ("https://zenn.dev/example", "znc", "zenn"),
        ] {
            let html = format!("<div class=\"{class}\"><p>{body}</p></div></div></div>");
            let (actual_method, actual_body) = extract_body(url, &html);
            assert_eq!(actual_method, method);
            assert_eq!(actual_body, body);
        }
    }

    #[test]
    fn pdf_cleanup_removes_repeated_short_lines_and_page_numbers() {
        let pages = vec![
            "共通見出し\n1\n第一本文".to_owned(),
            "共通見出し\n2\n第二本文".to_owned(),
            "共通見出し\n3\n第三本文".to_owned(),
        ];
        assert_eq!(clean_pdf_pages(&pages), "第一本文\n第二本文\n第三本文");
    }
}
