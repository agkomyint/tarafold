#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::net::TcpStream;

use super::*;

fn fixture() -> (Report, Dataset) {
    let e = tarafold_synth::find("linear").unwrap();
    let g = tarafold_synth::generate(&e, 200, 0.0, 0, 3).unwrap();
    let csv = g.to_csv().unwrap();
    let data = tarafold_io::read_csv(csv.as_bytes(), "linear.csv", &Default::default()).unwrap();
    let mut cfg = RunConfig {
        targets: vec!["y".into()],
        ..Default::default()
    };
    cfg.discover.gp.max_evals = 20_000;
    cfg.validate.stability = 0;
    cfg.validate.bootstrap = 20;
    let (r, _) = tarafold_engine::run(&data, &cfg).unwrap();
    (r, data)
}

fn server() -> Running {
    let (report, data) = fixture();
    start(ServeOptions {
        addr: "127.0.0.1:0".parse().unwrap(),
        report,
        data: Some(data),
        token: Some("00112233445566778899aabbccddeeff".into()),
    })
    .unwrap()
}

/// Minimal HTTP/1.1 client: returns (status, body).
fn http(
    r: &Running,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &str,
) -> (u16, String) {
    let mut s = TcpStream::connect(r.addr).unwrap();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    let has_host = headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host"));
    if !has_host {
        req.push_str(&format!("Host: {}\r\n", r.addr));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    let text = String::from_utf8_lossy(&out).to_string();
    let status = text.split(' ').nth(1).unwrap().parse().unwrap();
    let body = text
        .split_once("\r\n\r\n")
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    (status, body)
}

fn auth(r: &Running) -> (&'static str, String) {
    ("Authorization", format!("Bearer {}", r.token))
}

#[test]
fn token_host_and_origin_are_enforced() {
    let r = server();
    // No token.
    assert_eq!(http(&r, "GET", "/api/report", &[], "").0, 401);
    // Wrong token of the right length.
    let bad = (
        "Authorization",
        "Bearer ffffffffffffffffffffffffffffffff".to_string(),
    );
    assert_eq!(http(&r, "GET", "/api/report", &[bad], "").0, 401);
    // Right token.
    let (st, body) = http(&r, "GET", "/api/report", &[auth(&r)], "");
    assert_eq!(st, 200);
    assert!(body.contains("\"schema_version\""));
    // Foreign Origin (cross-site request) with the right token.
    let evil = ("Origin", "http://evil.example".to_string());
    assert_eq!(http(&r, "GET", "/api/report", &[auth(&r), evil], "").0, 403);
    // Same origin is fine.
    let same = ("Origin", format!("http://{}", r.addr));
    assert_eq!(http(&r, "GET", "/api/report", &[auth(&r), same], "").0, 200);
    // DNS rebinding: a foreign Host header.
    let host = ("Host", format!("attacker.example:{}", r.addr.port()));
    assert_eq!(http(&r, "GET", "/", &[host], "").0, 403);
    // The UI page itself needs no token.
    let (st, page) = http(&r, "GET", "/", &[], "");
    assert_eq!(st, 200);
    assert!(page.contains("<html"));
    r.stop().unwrap();
}

#[test]
fn body_limit_and_bad_requests() {
    let r = server();
    let big = format!("{{\"require\":[\"{}\"]}}", "x".repeat(BODY_LIMIT + 10));
    let ct = ("Content-Type", "application/json".to_string());
    assert_eq!(
        http(&r, "POST", "/api/optimize", &[auth(&r), ct.clone()], &big).0,
        413
    );
    let (st, body) = http(
        &r,
        "POST",
        "/api/optimize",
        &[auth(&r), ct.clone()],
        "{\"require\":[\"nope>=1\"]}",
    );
    assert_eq!(st, 400, "{body}");
    assert!(body.contains("error"));
    let (st, _) = http(
        &r,
        "POST",
        "/api/optimize",
        &[auth(&r), ct],
        "{\"unknown\":1}",
    );
    assert_eq!(st, 422);
    r.stop().unwrap();
}

#[test]
fn optimize_suggest_and_evaluate_without_rediscovery() {
    let r = server();
    let ct = ("Content-Type", "application/json".to_string());
    let (st, body) = http(
        &r,
        "POST",
        "/api/optimize",
        &[auth(&r), ct.clone()],
        "{\"require\":[\"y=20\"]}",
    );
    assert_eq!(st, 200, "{body}");
    let rep: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(!rep["optimize"]["candidates"].as_array().unwrap().is_empty());
    let (st, body) = http(
        &r,
        "POST",
        "/api/suggest",
        &[auth(&r), ct.clone()],
        "{\"n\":3,\"mode\":\"explore\"}",
    );
    assert_eq!(st, 200, "{body}");
    let rep: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(rep["suggest"]["suggestions"].as_array().unwrap().len(), 3);
    // The optimization from before is kept.
    assert!(rep["optimize"].is_object());
    let (st, body) = http(
        &r,
        "POST",
        "/api/evaluate",
        &[auth(&r), ct],
        "{\"points\":[{\"x1\":1,\"x2\":2}]}",
    );
    assert_eq!(st, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let p = v["predictions"][0].as_f64().unwrap();
    assert!((p - (3.0 - 4.0 + 5.0)).abs() < 1e-6, "{p}");
    let (st, body) = http(&r, "GET", "/api/data", &[auth(&r)], "");
    assert_eq!(st, 200);
    assert!(body.contains("\"columns\""));
    r.stop().unwrap();
}

#[test]
fn tokens_are_random_and_compared_safely() {
    let a = new_token().unwrap();
    let b = new_token().unwrap();
    assert_eq!(a.len(), 32);
    assert_ne!(a, b);
    assert!(token_ok(&a, &a));
    assert!(!token_ok(&a, &b));
    assert!(!token_ok("", &a));
}
