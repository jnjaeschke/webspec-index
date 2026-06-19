//! Native messaging host for the Firefox extension.
//!
//! Protocol: each message is a 4-byte little-endian length prefix + JSON payload.
//!
//! Request:  {"id": N, "url": "https://html.spec.whatwg.org/#navigate"}
//!           url accepts both full URLs and SPEC#anchor shorthand.
//!
//! Response (ok):
//!   {"id": N, "ok": true, "spec": "HTML", "anchor": "navigate",
//!    "title": "...", "section_type": "...", "content": "...",
//!    "parent": {"anchor": "...", "title": "..."},
//!    "outgoing_refs": [{"spec": "DOM", "anchor": "concept-tree"}, ...]}
//!
//! Response (error):
//!   {"id": N, "ok": false, "error": "..."}

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Request {
    #[serde(rename = "query")]
    Query { id: u32, url: String },
    #[serde(rename = "search")]
    Search {
        id: u32,
        spec: String,
        query: String,
    },
    #[serde(rename = "list")]
    List { id: u32, spec: String },
}

#[derive(Serialize)]
struct HeadingItem {
    anchor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    depth: u8,
}

#[derive(Serialize)]
struct SearchItem {
    spec: String,
    anchor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    section_type: String,
}

#[derive(Serialize)]
struct RefItem {
    spec: String,
    anchor: String,
}

#[derive(Serialize)]
struct ParentInfo {
    anchor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

#[derive(Serialize)]
struct Response {
    id: u32,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    section_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<ParentInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    outgoing_refs: Vec<RefItem>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    search_results: Vec<SearchItem>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    headings: Vec<HeadingItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn read_message(stdin: &mut tokio::io::Stdin) -> anyhow::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match stdin.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    stdin.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

async fn write_message(stdout: &mut tokio::io::Stdout, data: &[u8]) -> anyhow::Result<()> {
    let len = (data.len() as u32).to_le_bytes();
    stdout.write_all(&len).await?;
    stdout.write_all(data).await?;
    stdout.flush().await?;
    Ok(())
}

pub async fn serve() {
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    loop {
        let buf = match read_message(&mut stdin).await {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) => {
                eprintln!("native-messaging: read error: {e}");
                break;
            }
        };

        let req: Request = match serde_json::from_slice(&buf) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("native-messaging: parse error: {e}");
                continue;
            }
        };

        let response = match req {
            Request::Query { id, url } => match crate::query_section(&url, None).await {
                Ok(result) => Response {
                    id,
                    ok: true,
                    spec: Some(result.spec),
                    anchor: Some(result.anchor),
                    title: result.title,
                    section_type: Some(result.section_type),
                    content: result.content,
                    parent: result.navigation.parent.map(|p| ParentInfo {
                        anchor: p.anchor,
                        title: p.title,
                    }),
                    outgoing_refs: result
                        .outgoing_refs
                        .into_iter()
                        .map(|r| RefItem {
                            spec: r.spec,
                            anchor: r.anchor,
                        })
                        .collect(),
                    search_results: vec![],
                    headings: vec![],
                    error: None,
                },
                Err(e) => Response {
                    id,
                    ok: false,
                    spec: None,
                    anchor: None,
                    title: None,
                    section_type: None,
                    content: None,
                    parent: None,
                    outgoing_refs: vec![],
                    search_results: vec![],
                    headings: vec![],
                    error: Some(e.to_string()),
                },
            },
            Request::Search { id, spec, query } => {
                match crate::search_sections(&query, Some(&spec), 10, None).await {
                    Ok(result) => Response {
                        id,
                        ok: true,
                        spec: None,
                        anchor: None,
                        title: None,
                        section_type: None,
                        content: None,
                        parent: None,
                        outgoing_refs: vec![],
                        search_results: result
                            .results
                            .into_iter()
                            .map(|e| SearchItem {
                                spec: e.spec,
                                anchor: e.anchor,
                                title: e.title,
                                section_type: e.section_type,
                            })
                            .collect(),
                        headings: vec![],
                        error: None,
                    },
                    Err(e) => Response {
                        id,
                        ok: false,
                        spec: None,
                        anchor: None,
                        title: None,
                        section_type: None,
                        content: None,
                        parent: None,
                        outgoing_refs: vec![],
                        search_results: vec![],
                        headings: vec![],
                        error: Some(e.to_string()),
                    },
                }
            }
            Request::List { id, spec } => match crate::list_headings(&spec, None).await {
                Ok(entries) => Response {
                    id,
                    ok: true,
                    spec: None,
                    anchor: None,
                    title: None,
                    section_type: None,
                    content: None,
                    parent: None,
                    outgoing_refs: vec![],
                    search_results: vec![],
                    headings: entries
                        .into_iter()
                        .filter(|e| e.depth > 0)
                        .map(|e| HeadingItem {
                            anchor: e.anchor,
                            title: e.title,
                            depth: e.depth,
                        })
                        .collect(),
                    error: None,
                },
                Err(e) => Response {
                    id,
                    ok: false,
                    spec: None,
                    anchor: None,
                    title: None,
                    section_type: None,
                    content: None,
                    parent: None,
                    outgoing_refs: vec![],
                    search_results: vec![],
                    headings: vec![],
                    error: Some(e.to_string()),
                },
            },
        };

        let json = match serde_json::to_vec(&response) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("native-messaging: serialize error: {e}");
                continue;
            }
        };

        if let Err(e) = write_message(&mut stdout, &json).await {
            eprintln!("native-messaging: write error: {e}");
            break;
        }
    }
}
