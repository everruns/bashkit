//! Record an in-process terminal session as an asciicast v2 file.
//!
//! Types a short scenario key by key (shell, `vi` edit, `less`), collects the
//! raw output stream from `Terminal::take_output()`, and writes it with
//! synthetic timestamps. Replay with `asciinema play FILE` or embed it with
//! asciinema-player.
//!
//! Run with: cargo run --example terminal_record --features terminal -- [OUT.cast]

use bashkit::Bash;
use bashkit::terminal::{Terminal, TerminalSize, TerminalStatus};

/// Delay between typed keys, and after a step completes.
const KEY_DELAY: f64 = 0.07;
const STEP_PAUSE: f64 = 1.2;

struct Recorder {
    term: Terminal,
    clock: f64,
    events: Vec<String>,
}

impl Recorder {
    fn capture(&mut self) {
        let out = self.term.take_output();
        if !out.is_empty() {
            let text = String::from_utf8_lossy(&out);
            let data = serde_json::to_string(&text).unwrap();
            self.events
                .push(format!("[{:.3}, \"o\", {data}]", self.clock));
        }
    }

    /// Type `keys` one character at a time, then pause so a viewer can read.
    async fn type_keys(&mut self, keys: &str) {
        for ch in keys.chars() {
            self.term.send(ch.to_string());
            self.term.run_until_idle().await;
            self.clock += KEY_DELAY;
            self.capture();
        }
        self.clock += STEP_PAUSE;
    }
}

#[tokio::main]
async fn main() {
    let out_path = std::env::args().nth(1).unwrap_or_else(|| {
        std::env::temp_dir()
            .join("terminal-demo.cast")
            .display()
            .to_string()
    });
    let size = TerminalSize::new(16, 72);
    let mut rec = Recorder {
        term: Terminal::with_size(Bash::builder(), size),
        clock: 0.0,
        events: Vec::new(),
    };
    rec.term.run_until_idle().await;
    rec.capture();
    rec.clock += STEP_PAUSE;

    rec.type_keys("printf 'name = \"demo\"\\nport = 8080\\ndebug = true\\n' > app.toml\r")
        .await;
    rec.type_keys("vi app.toml\r").await;
    rec.type_keys("/8080\r").await;
    rec.type_keys("cw9090\x1b").await;
    rec.type_keys(":%s/true/false/\r").await;
    rec.type_keys("Go# edited in vi\x1b").await;
    rec.type_keys(":wq\r").await;
    rec.type_keys("cat app.toml\r").await;
    rec.type_keys("seq 1 200 | less\r").await;
    rec.type_keys(" ").await;
    rec.type_keys("/150\r").await;
    rec.type_keys("q").await;
    rec.type_keys("exit\r").await;
    assert_eq!(rec.term.run_until_idle().await, TerminalStatus::Exited(0));

    let saved = rec
        .term
        .fs()
        .read_file("/home/user/app.toml".as_ref())
        .await
        .unwrap();
    assert_eq!(
        String::from_utf8(saved).unwrap(),
        "name = \"demo\"\nport = 9090\ndebug = false\n# edited in vi\n"
    );

    let header = format!(
        "{{\"version\": 2, \"width\": {}, \"height\": {}, \"env\": {{\"TERM\": \"xterm-256color\"}}, \"title\": \"bashkit in-process terminal\"}}",
        size.cols, size.rows
    );
    let mut cast = header;
    for event in &rec.events {
        cast.push('\n');
        cast.push_str(event);
    }
    cast.push('\n');
    std::fs::write(&out_path, cast).unwrap();
    println!("wrote {} events to {out_path}", rec.events.len());
}
