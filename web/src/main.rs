use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> Result<()> {
    let addr = "127.0.0.1:8080";
    let listener = TcpListener::bind(addr).with_context(|| format!("failed to bind {addr}"))?;
    println!("Brevis Vera web app running at http://{addr}");

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                if let Err(err) = handle_connection(&mut stream) {
                    let _ = write_response(
                        &mut stream,
                        500,
                        "text/plain; charset=utf-8",
                        format!("internal error: {err}\n").as_bytes(),
                    );
                }
            }
            Err(err) => eprintln!("connection error: {err}"),
        }
    }

    Ok(())
}

fn handle_connection(stream: &mut TcpStream) -> Result<()> {
    let req = read_request(stream)?;
    let (path, _) = split_path_query(&req.path);

    match (req.method.as_str(), path.as_str()) {
        ("GET", "/") => serve_static_file(stream, "index.html")?,
        ("GET", "/verify-page") => serve_static_file(stream, "verify.html")?,
        ("GET", p) if p.starts_with("/assets/") => {
            serve_static_file(stream, p.trim_start_matches('/'))?
        }
        ("GET", p) if p.starts_with("/files/") => {
            serve_workspace_file(stream, p.trim_start_matches("/files/"))?
        }
        ("POST", "/api/run-pipeline") => {
            let form = parse_form(&req.body)?;
            let payload = api_run_pipeline(&form)?;
            write_json_response(stream, 200, &payload)?;
        }
        ("POST", "/api/verify-artifacts") => {
            let form = parse_form(&req.body)?;
            let payload = api_verify_artifacts(&form)?;
            write_json_response(stream, 200, &payload)?;
        }
        ("POST", "/api/verify-c2pa") => {
            let form = parse_form(&req.body)?;
            let payload = api_verify_c2pa(&form)?;
            write_json_response(stream, 200, &payload)?;
        }
        ("POST", "/api/upload-image") => {
            let uploaded_path = handle_upload_image(&req)?;
            write_json_response(stream, 200, &json!({"path": uploaded_path}))?;
        }
        ("GET", "/api/health") => {
            write_json_response(stream, 200, &json!({"ok": true}))?;
        }
        _ => {
            write_response(
                stream,
                404,
                "text/plain; charset=utf-8",
                b"not found\n",
            )?;
        }
    }

    Ok(())
}

fn api_run_pipeline(form: &HashMap<String, String>) -> Result<Value> {
    let input_image = value_or(form, "input_image", "samples/input.png");
    let run_dir = create_run_dir()?;
    let metadata_out = format!("{run_dir}/mock_metadata.json");
    let edited_out = format!("{run_dir}/edited.png");
    let proof_out = format!("{run_dir}/riscv_proof.bin");
    let pv_out = format!("{run_dir}/public_values.json");

    let crop_x = value_or(form, "crop_x", "1");
    let crop_y = value_or(form, "crop_y", "1");
    let crop_w = value_or(form, "crop_w", "4");
    let crop_h = value_or(form, "crop_h", "4");
    let brightness = value_or(form, "brightness_delta", "20");
    let rotate = value_or(form, "rotate_quarters", "0");
    let threshold = value_or(form, "threshold", "");
    let invert = form.contains_key("invert");

    let sign = run_cargo_prover(&[
        "mock-sign",
        "--input-image",
        &input_image,
        "--metadata-out",
        &metadata_out,
        "--private-key-pem",
        "artifacts/mock_signer_key.pem",
    ])?;

    let prove = if sign.status_ok {
        let mut prove_args = vec![
            "edit-and-prove",
            "--input-image",
            &input_image,
            "--metadata",
            &metadata_out,
            "--crop-x",
            &crop_x,
            "--crop-y",
            &crop_y,
            "--crop-w",
            &crop_w,
            "--crop-h",
            &crop_h,
            "--brightness-delta",
            &brightness,
            "--rotate-quarters",
            &rotate,
            "--edited-image-out",
            &edited_out,
            "--riscv-proof-out",
            &proof_out,
            "--public-values-out",
            &pv_out,
        ];
        if invert {
            prove_args.push("--invert");
        }
        if !threshold.trim().is_empty() {
            prove_args.push("--threshold");
            prove_args.push(&threshold);
        }
        run_cargo_prover(&prove_args)?
    } else {
        CmdResult {
            status_ok: false,
            stdout: String::new(),
            stderr: "skipped: mock-sign failed".to_string(),
        }
    };

    Ok(json!({
        "ok": sign.status_ok && prove.status_ok,
        "run_dir": run_dir,
        "artifacts": {
            "metadata": metadata_out,
            "edited_image": edited_out,
            "riscv_proof": proof_out,
            "public_values": pv_out
        },
        "steps": [
            cmd_json("mock-sign", &sign),
            cmd_json("edit-and-prove", &prove)
        ]
    }))
}

fn api_verify_artifacts(form: &HashMap<String, String>) -> Result<Value> {
    let edited_image = value_or(form, "edited_image", "");
    let metadata = value_or(form, "metadata", "");
    let riscv_proof = value_or(form, "riscv_proof", "");
    let public_values = value_or(form, "public_values", "");

    if edited_image.trim().is_empty() || metadata.trim().is_empty() || riscv_proof.trim().is_empty() {
        bail!("edited_image, metadata and riscv_proof are required");
    }

    let verify = run_cargo_prover(&[
        "verify",
        "--edited-image",
        &edited_image,
        "--metadata",
        &metadata,
        "--riscv-proof",
        &riscv_proof,
    ])?;

    let public_values_text = if public_values.trim().is_empty() {
        None
    } else {
        Some(
            fs::read_to_string(&public_values)
                .with_context(|| format!("failed to read public values file: {public_values}"))?,
        )
    };

    Ok(json!({
        "ok": verify.status_ok,
        "artifacts": {
            "edited_image": edited_image,
            "metadata": metadata,
            "riscv_proof": riscv_proof,
            "public_values": public_values
        },
        "verify": cmd_json("verify", &verify),
        "public_values_text": public_values_text
    }))
}

fn api_verify_c2pa(form: &HashMap<String, String>) -> Result<Value> {
    let input_image = value_or(form, "input_image", "DSC00050.JPG");
    let verify = run_cargo_prover(&["verify-c2pa", "--input-image", &input_image])?;
    Ok(json!({
        "ok": verify.status_ok,
        "input_image": input_image,
        "verify": cmd_json("verify-c2pa", &verify)
    }))
}

fn cmd_json(name: &str, cmd: &CmdResult) -> Value {
    json!({
        "name": name,
        "ok": cmd.status_ok,
        "stdout": cmd.stdout,
        "stderr": cmd.stderr
    })
}

struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    let mut buf = Vec::new();
    let mut temp = [0_u8; 4096];
    let header_end;

    loop {
        let n = stream.read(&mut temp).context("failed to read request")?;
        if n == 0 {
            bail!("empty request");
        }
        buf.extend_from_slice(&temp[..n]);

        if let Some(pos) = find_header_end(&buf) {
            header_end = pos;
            break;
        }
        if buf.len() > 1024 * 1024 {
            bail!("request headers too large");
        }
    }

    let headers_raw = &buf[..header_end];
    let mut body = buf[header_end + 4..].to_vec();

    let headers_str = String::from_utf8(headers_raw.to_vec()).context("invalid request headers utf8")?;
    let mut lines = headers_str.lines();
    let request_line = lines.next().context("missing request line")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().context("missing method")?.to_string();
    let path = parts.next().context("missing path")?.to_string();

    let mut content_length = 0usize;
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let value = v.trim().to_string();
            if key == "content-length" {
                content_length = value.parse::<usize>().context("invalid content-length")?;
            }
            headers.insert(key, value);
        }
    }

    while body.len() < content_length {
        let n = stream.read(&mut temp).context("failed to read request body")?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&temp[..n]);
    }
    body.truncate(content_length);

    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn split_path_query(path: &str) -> (String, String) {
    if let Some((left, right)) = path.split_once('?') {
        (left.to_string(), right.to_string())
    } else {
        (path.to_string(), String::new())
    }
}

fn parse_form(body: &[u8]) -> Result<HashMap<String, String>> {
    let s = String::from_utf8(body.to_vec()).context("form body is not utf8")?;
    parse_kv_pairs(&s)
}

fn parse_kv_pairs(s: &str) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for pair in s.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.insert(url_decode(k)?, url_decode(v)?);
    }
    Ok(out)
}

fn url_decode(s: &str) -> Result<String> {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                let val = u8::from_str_radix(hex, 16)
                    .with_context(|| format!("invalid percent escape: %{hex}"))?;
                out.push(val as char);
                i += 3;
            }
            b => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    Ok(out)
}

fn write_response(stream: &mut TcpStream, code: u16, content_type: &str, body: &[u8]) -> Result<()> {
    let status = match code {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {code} {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

fn write_json_response(stream: &mut TcpStream, code: u16, value: &Value) -> Result<()> {
    let body = serde_json::to_vec_pretty(value).context("failed to serialize json")?;
    write_response(stream, code, "application/json; charset=utf-8", &body)
}

fn serve_static_file(stream: &mut TcpStream, rel: &str) -> Result<()> {
    let root = std::env::current_dir()?.join("web/frontend");
    let path = if rel == "/" {
        root.join("index.html")
    } else {
        root.join(rel)
    };
    serve_from_root(stream, &root, &path)
}

fn serve_workspace_file(stream: &mut TcpStream, rel: &str) -> Result<()> {
    let root = std::env::current_dir()?;
    let path = root.join(rel);
    serve_from_root(stream, &root, &path)
}

fn serve_from_root(stream: &mut TcpStream, root: &Path, full: &Path) -> Result<()> {
    let canon = match full.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            write_response(stream, 404, "text/plain; charset=utf-8", b"file not found\n")?;
            return Ok(());
        }
    };

    if !canon.starts_with(root) {
        write_response(stream, 404, "text/plain; charset=utf-8", b"forbidden\n")?;
        return Ok(());
    }

    let body = fs::read(&canon).context("failed reading file")?;
    let content_type = guess_content_type(&canon);
    write_response(stream, 200, content_type, &body)
}

fn handle_upload_image(req: &HttpRequest) -> Result<String> {
    let content_type = req
        .headers
        .get("content-type")
        .context("missing content-type for upload")?;
    let boundary = parse_boundary(content_type).context("missing multipart boundary")?;
    let (filename, bytes) = extract_multipart_file(&req.body, &boundary)?;

    let ext = Path::new(&filename)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| matches!(s.as_str(), "png" | "jpg" | "jpeg"))
        .unwrap_or_else(|| "bin".to_string());

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let dir = Path::new("artifacts/web_uploads");
    fs::create_dir_all(dir).context("failed to create upload directory")?;
    let rel_path = format!("artifacts/web_uploads/upload_{ts}.{ext}");
    fs::write(&rel_path, bytes)
        .with_context(|| format!("failed writing uploaded file to {rel_path}"))?;
    Ok(rel_path)
}

fn parse_boundary(content_type: &str) -> Option<String> {
    for part in content_type.split(';') {
        let p = part.trim();
        if let Some(value) = p.strip_prefix("boundary=") {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

fn extract_multipart_file(body: &[u8], boundary: &str) -> Result<(String, Vec<u8>)> {
    let marker = format!("--{boundary}");
    let marker_bytes = marker.as_bytes();
    if !body.starts_with(marker_bytes) {
        bail!("multipart body does not start with boundary");
    }

    let mut cursor = marker_bytes.len();
    if !body[cursor..].starts_with(b"\r\n") {
        bail!("invalid multipart delimiter");
    }
    cursor += 2;

    let header_end_rel =
        find_bytes(&body[cursor..], b"\r\n\r\n").context("multipart headers not found")?;
    let headers = &body[cursor..cursor + header_end_rel];
    cursor += header_end_rel + 4;

    let headers_str =
        String::from_utf8(headers.to_vec()).context("invalid multipart headers utf8")?;
    let mut filename = "upload.bin".to_string();
    for line in headers_str.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("content-disposition:") {
            if let Some((_, right)) = line.split_once("filename=") {
                filename = right.trim().trim_matches('"').to_string();
            }
        }
    }
    filename = sanitize_filename(&filename);

    let ending = format!("\r\n--{boundary}");
    let end_rel =
        find_bytes(&body[cursor..], ending.as_bytes()).context("multipart file end not found")?;
    let file_bytes = body[cursor..cursor + end_rel].to_vec();
    Ok((filename, file_bytes))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn sanitize_filename(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            out.push(ch);
        }
    }
    if out.is_empty() {
        "upload.bin".to_string()
    } else {
        out
    }
}

fn guess_content_type(path: &PathBuf) -> &'static str {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "json" => "application/json; charset=utf-8",
        "bin" => "application/octet-stream",
        "md" => "text/markdown; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn value_or(form: &HashMap<String, String>, key: &str, default: &str) -> String {
    form.get(key)
        .map(String::as_str)
        .unwrap_or(default)
        .to_string()
}

fn create_run_dir() -> Result<String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let dir = format!("artifacts/web_runs/run_{ts}");
    fs::create_dir_all(&dir).with_context(|| format!("failed to create run directory: {dir}"))?;
    Ok(dir)
}

#[derive(Clone)]
struct CmdResult {
    status_ok: bool,
    stdout: String,
    stderr: String,
}

fn run_cargo_prover(args: &[&str]) -> Result<CmdResult> {
    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .arg("--release")
        .arg("-p")
        .arg("brevis-vera-prover")
        .arg("--");
    cmd.args(args);
    cmd.current_dir(std::env::current_dir()?);

    let out = cmd.output().context("failed to run cargo prover command")?;
    Ok(CmdResult {
        status_ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    })
}
