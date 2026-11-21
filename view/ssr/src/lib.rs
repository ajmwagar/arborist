//! `arborist.myc`: the fleet's source health, server-rendered.
//!
//! The Spore owns the HTML and every projection under it. The host owns
//! routing and the VFS. A snapshot mounted at [`SNAPSHOT_PATH`] is evaluated
//! here with the same `arborist::evaluate` the command uses; when none is
//! mounted the fixture compiled into the module is rendered and the page says
//! so. There is no second evaluator, which is the point.

wit_bindgen::generate!({
    path: "wit",
    world: "arborist-ui",
    generate_all,
});

use wasi::http::types::{
    Fields, IncomingRequest, Method, OutgoingBody, OutgoingResponse, ResponseOutparam,
};

use soil::vfs::vfs;

mod render;

use crate::render::Source;

/// The document a host may mount. `arborist ingest` writes a snapshot; the page
/// evaluates it, so a report and a page can never disagree.
const SNAPSHOT_PATH: &str = "snapshot.json";
/// The report a host may mount instead, already evaluated by the command.
const REPORT_PATH: &str = "report.json";
/// Sixteen megabytes of snapshot is a fleet far larger than this one; a range
/// over that is a mistake, not a page worth rendering.
const MAX_BYTES: usize = 16 * 1024 * 1024;
const CHUNK: u32 = 1024 * 1024;
/// The fixture has the real fleet's shape and invented names, because this
/// repository is public and the fleet is not.
const FIXTURE: &str = include_str!("../../../fixtures/fleet.json");

struct Component;

export!(Component);

impl exports::wasi::http::incoming_handler::Guest for Component {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        let method = request.method();
        let target = request.path_with_query().unwrap_or_else(|| "/".to_string());
        let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));

        if !matches!(method, Method::Get) {
            answer(
                response_out,
                405,
                "text/plain; charset=utf-8",
                b"method not allowed\n",
            );
            return;
        }
        if path == "/healthz" {
            answer(response_out, 200, "text/plain; charset=utf-8", b"ok\n");
            return;
        }

        let (report, source) = match load() {
            Ok(loaded) => loaded,
            Err(error) => {
                answer(
                    response_out,
                    503,
                    "text/html; charset=utf-8",
                    render::error_page(&error).as_bytes(),
                );
                return;
            }
        };

        let (status, content_type, body) = match path {
            "/" => (
                200,
                "text/html; charset=utf-8",
                render::dashboard(&report, &source),
            ),
            "/checks" => (
                200,
                "text/html; charset=utf-8",
                render::checks_page(&report, &source),
            ),
            "/repo" => {
                let org = query_value(query, "org").unwrap_or_default();
                let name = query_value(query, "name").unwrap_or_default();
                match render::repo_page(&report, &source, &org, &name) {
                    Some(page) => (200, "text/html; charset=utf-8", page),
                    None => (
                        404,
                        "text/html; charset=utf-8",
                        render::error_page(&format!("no repository {org}/{name} in this report")),
                    ),
                }
            }
            "/report.json" => (
                200,
                "application/json; charset=utf-8",
                match serde_json::to_string_pretty(&report) {
                    Ok(json) => json,
                    Err(error) => format!("{{\"error\":\"{error}\"}}"),
                },
            ),
            _ => (
                404,
                "text/html; charset=utf-8",
                render::error_page(&format!("no route for {path}")),
            ),
        };
        answer(response_out, status, content_type, body.as_bytes());
    }
}

/// Read the mounted snapshot or report, falling back to the fixture.
fn load() -> Result<(arborist::Report, Source), String> {
    match read(SNAPSHOT_PATH) {
        Ok(Some(bytes)) => {
            let snapshot: arborist::Snapshot = serde_json::from_slice(&bytes)
                .map_err(|error| format!("parsing {SNAPSHOT_PATH}: {error}"))?;
            return Ok((
                arborist::evaluate(&snapshot),
                Source::Snapshot(SNAPSHOT_PATH.to_string()),
            ));
        }
        Ok(None) => {}
        Err(error) => return Err(format!("reading {SNAPSHOT_PATH}: {error}")),
    }

    match read(REPORT_PATH) {
        Ok(Some(bytes)) => {
            let report: arborist::Report = serde_json::from_slice(&bytes)
                .map_err(|error| format!("parsing {REPORT_PATH}: {error}"))?;
            return Ok((report, Source::Report(REPORT_PATH.to_string())));
        }
        Ok(None) => {}
        Err(error) => return Err(format!("reading {REPORT_PATH}: {error}")),
    }

    let snapshot: arborist::Snapshot = serde_json::from_str(FIXTURE)
        .map_err(|error| format!("the compiled-in fixture is not a snapshot: {error}"))?;
    Ok((arborist::evaluate(&snapshot), Source::Fixture))
}

/// Read a whole VFS object, or `None` when the host has no such path.
fn read(path: &str) -> Result<Option<Vec<u8>>, String> {
    match vfs::stat(path) {
        Ok(entry) => {
            if entry.kind == vfs::Kind::Directory {
                return Err(format!("{path} is a directory"));
            }
            if entry.size > MAX_BYTES as u64 {
                return Err(format!(
                    "{path} is {} bytes, over the {MAX_BYTES} byte page budget",
                    entry.size
                ));
            }
        }
        Err(vfs::Error::NotFound) => return Ok(None),
        Err(error) => return Err(format!("{error:?}")),
    }

    let mut out = Vec::new();
    let mut offset = 0u64;
    loop {
        let chunk = vfs::read(path, offset, CHUNK).map_err(|error| format!("{error:?}"))?;
        out.extend_from_slice(&chunk.data);
        if out.len() > MAX_BYTES {
            return Err(format!("{path} grew past the {MAX_BYTES} byte page budget"));
        }
        if chunk.eof || chunk.data.is_empty() {
            break;
        }
        offset += chunk.data.len() as u64;
    }
    Ok(Some(out))
}

fn answer(response_out: ResponseOutparam, status: u16, content_type: &str, body: &[u8]) {
    let fields =
        Fields::from_list(&[("content-type".to_string(), content_type.as_bytes().to_vec())])
            .unwrap_or_else(|_| Fields::new());
    let _ = fields.set("cache-control", &[b"no-store".to_vec()]);
    let response = OutgoingResponse::new(fields);
    if response.set_status_code(status).is_err() {
        return;
    }
    let Ok(outgoing) = response.body() else {
        return;
    };
    ResponseOutparam::set(response_out, Ok(response));
    let Ok(stream) = outgoing.write() else {
        return;
    };
    for chunk in body.chunks(4096) {
        if stream.blocking_write_and_flush(chunk).is_err() {
            break;
        }
    }
    drop(stream);
    let _ = OutgoingBody::finish(outgoing, None);
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                if let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    out.push(byte);
                    index += 3;
                } else {
                    out.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
