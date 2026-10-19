//! Opt-in integration driver: real isolated host, no app/profile initialization.
use serde::Deserialize;
use std::path::PathBuf;
#[derive(Deserialize)]
struct Case {
    path: PathBuf,
    #[serde(default)]
    page: u32,
    expected: String,
    #[serde(default)]
    contains: Option<String>,
    #[serde(default)]
    cancel_after_ms: Option<u64>,
}
pub fn run(manifest: &str) -> i32 {
    let result = (|| -> anyhow::Result<bool> {
        let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(manifest)?)?;
        let mut output = Vec::new();
        let mut passed = true;
        for case in cases {
            let meta = std::fs::metadata(&case.path)?;
            let key = crate::document_preview::Key {
                file: crate::media::Key {
                    path: case.path.clone(),
                    mtime: 0,
                    size: meta.len(),
                    edge: crate::media::PREVIEW_EDGE,
                    details: false,
                    folder: false,
                },
                page: case.page,
            };
            let started = std::time::Instant::now();
            let result = crate::document_preview::load(&key, &|| {
                case.cancel_after_ms
                    .is_some_and(|ms| started.elapsed().as_millis() >= ms as u128)
            });
            let check = match &result {
                Ok(preview) => {
                    let source = &preview.info.source;
                    let content = preview.info.text.clone().unwrap_or_else(|| {
                        preview
                            .info
                            .sheet
                            .as_ref()
                            .map(|s| {
                                s.rows
                                    .iter()
                                    .flatten()
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .unwrap_or_default()
                    });
                    (source == &case.expected
                        || case.expected == "Word" && source.starts_with("Word"))
                        && case.contains.as_ref().is_none_or(|s| content.contains(s))
                }
                Err(e) => {
                    case.expected == "unavailable"
                        || case.expected == "cancelled" && e == "Cancelled"
                }
            };
            passed &= check;
            output.push(serde_json::json!({"name":case.path.file_name().map(|n|n.to_string_lossy()),"page":case.page,"passed":check,"elapsed_ms":started.elapsed().as_secs_f64()*1000.,"source":result.as_ref().ok().map(|p|&p.info.source),"dimensions":result.as_ref().ok().map(|p|(p.info.width,p.info.height)),"pages":result.as_ref().ok().map(|p|p.info.pages),"text_chars":result.as_ref().ok().and_then(|p|p.info.text.as_ref().map(|t|t.chars().count())),"sheet":result.as_ref().ok().and_then(|p|p.info.sheet.as_ref().map(|s|serde_json::json!({"name":s.name,"shown_rows":s.rows.len(),"total_rows":s.total_rows,"total_cols":s.total_cols}))),"error":result.as_ref().err()}));
        }
        println!("{}", serde_json::to_string_pretty(&output)?);
        Ok(passed)
    })();
    match result {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
