//! A PDF read by `pdf-inspector` in a child of the real binary.
//!
//! Extract runs `pdf-inspector` in a child of this program, started with a flag `main` has
//! to hand to the reader before it parses a command line. If it stops doing that, nothing
//! fails: the child refuses the flag, the reader counts as having produced nothing, and
//! `pdftotext` reads every PDF instead, as flat text. So this runs the binary on a PDF and
//! asks which tool read it.

use std::process::{Command, Stdio};

/// A one-page PDF with one line of text in a standard font, which both PDF readers can
/// read: built here so the bytes sit beside the assertion.
fn council_pdf(line: &str) -> Vec<u8> {
    let content = format!("BT /F1 12 Tf 72 720 Td ({line}) Tj ET");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
            .to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];

    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

#[test]
fn a_pdf_is_read_by_pdf_inspector_in_its_child() {
    let line = "The council meets on Tuesdays at the City Hall annex.";
    let root = tempfile::tempdir().unwrap();
    let pdf = root.path().join("council.pdf");
    std::fs::write(&pdf, council_pdf(line)).unwrap();

    // `check`, because it runs the extractor `extract` runs and reports the tool, and needs
    // no source. The binary on this directory and nothing else: no config of the person
    // running the tests.
    let out = Command::new(env!("CARGO_BIN_EXE_centinel"))
        .arg("--root")
        .arg(root.path().join("store"))
        .args(["--json", "check", "--out"])
        .arg(root.path().join("out"))
        .arg(&pdf)
        .current_dir(root.path())
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path())
        .env_remove("CENTINEL_ROOT")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let doc = &report["documents"][0];
    assert!(
        doc["tool"]
            .as_str()
            .is_some_and(|t| t.starts_with("pdf-inspector")),
        "the primary did not read it in its child: {doc:#}"
    );
    assert!(
        doc["preview"].as_str().is_some_and(|p| p.contains(line)),
        "{doc:#}"
    );
}
