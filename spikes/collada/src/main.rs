//! usage: collada_spike [--runs N] <file.dae>...   (one JSON object per line)

fn main() {
    let mut args = std::env::args().skip(1).peekable();
    let mut runs = 5;
    let mut files = vec![];
    while let Some(a) = args.next() {
        if a == "--runs" {
            runs = args.next().and_then(|n| n.parse().ok()).unwrap_or(5);
        } else {
            files.push(a);
        }
    }
    for f in files {
        // mesh-loader can panic on malformed input (todo!/panic! paths).
        let res = std::panic::catch_unwind(|| collada_spike::summary::summarize(f.as_ref(), runs));
        let line = match res {
            Ok(Ok(s)) => serde_json::to_string(&s).unwrap(),
            Ok(Err(e)) => serde_json::json!({"file": f, "loader": "mesh-loader-0.1.13", "error": e.to_string()}).to_string(),
            Err(_) => serde_json::json!({"file": f, "loader": "mesh-loader-0.1.13", "error": "panic"}).to_string(),
        };
        println!("{line}");
    }
}
