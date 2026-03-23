//! Compare two CU metrics JSON files and print an aligned terminal-style table to stdout.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process;

use serde_json::Value;

fn usage() -> ! {
    eprintln!(
        "Usage: compare-metrics [--title TITLE] [--no-color] <baseline.json> <current.json>\n\
         Compare compute-unit metrics emitted by integration tests.\n\
         \n\
         Prints an aligned table. ANSI colors are used when stdout is a terminal unless\n\
         NO_COLOR is set or --no-color is passed."
    );
    process::exit(2);
}

struct Args {
    baseline: PathBuf,
    current: PathBuf,
    title: String,
    force_no_color: bool,
}

fn parse_args() -> Args {
    let mut args = env::args().skip(1);
    let mut title = "CU metrics comparison".to_string();
    let mut force_no_color = false;
    let mut positional: Vec<String> = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--title" => {
                title = args.next().unwrap_or_else(|| {
                    eprintln!("compare-metrics: --title requires a value");
                    process::exit(2);
                });
            }
            "--no-color" => force_no_color = true,
            s if s.starts_with("--") || s.starts_with('-') => {
                eprintln!("compare-metrics: unknown option: {s}");
                process::exit(2);
            }
            _ => positional.push(a),
        }
    }
    if positional.len() != 2 {
        usage();
    }
    Args {
        baseline: PathBuf::from(&positional[0]),
        current: PathBuf::from(&positional[1]),
        title,
        force_no_color,
    }
}

fn load_object_map(path: &PathBuf) -> std::collections::HashMap<String, Value> {
    let s = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("compare-metrics: read {}: {e}", path.display());
        process::exit(1);
    });
    let v: Value = serde_json::from_str(&s).unwrap_or_else(|e| {
        eprintln!("compare-metrics: parse {}: {e}", path.display());
        process::exit(1);
    });
    match v {
        Value::Object(o) => {
            for (k, v) in &o {
                let valid = v
                    .as_object()
                    .and_then(|m| m.get("compute_units_consumed"))
                    .and_then(Value::as_u64)
                    .is_some();
                if !valid {
                    eprintln!(
                        "compare-metrics: {}: key {k:?} is missing numeric `compute_units_consumed`",
                        path.display()
                    );
                    process::exit(1);
                }
            }
            o.into_iter().collect()
        }
        _ => {
            eprintln!(
                "compare-metrics: {}: expected JSON object at root",
                path.display()
            );
            process::exit(1);
        }
    }
}

fn cu(entry: Option<&Value>) -> Option<u64> {
    entry?.get("compute_units_consumed")?.as_u64()
}

fn fmt_u64(v: Option<u64>) -> String {
    match v {
        None => "—".to_string(),
        Some(n) => format_with_commas_u64(n),
    }
}

fn insert_commas_digit_string(s: String) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_with_commas_u64(n: u64) -> String {
    insert_commas_digit_string(n.to_string())
}

fn format_delta_i128(delta: i128) -> String {
    let sign = if delta < 0 { "-" } else { "+" };
    let n = delta.unsigned_abs();
    format!("{sign}{}", insert_commas_digit_string(n.to_string()))
}

fn display_width(s: &str) -> usize {
    s.chars().count()
}

fn pad_right(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        truncate_chars(s, width)
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

fn pad_left(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        truncate_chars(s, width)
    } else {
        format!("{}{s}", " ".repeat(width - w))
    }
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if display_width(s) <= max_chars {
        return s.to_string();
    }
    let take = max_chars.saturating_sub(3);
    let mut out: String = s.chars().take(take).collect();
    out.push_str("...");
    out
}

// https://no-color.org/
fn no_color_requested() -> bool {
    env::var_os("NO_COLOR").is_some()
}

#[derive(Clone, Copy)]
enum DeltaKind {
    Improved,
    Worse,
    Unchanged,
    NewKey,
    Removed,
    Missing,
}

struct TableRow {
    key: String,
    baseline: String,
    current: String,
    delta: String,
    pct: String,
    kind: DeltaKind,
}

fn build_rows(
    a: &std::collections::HashMap<String, Value>,
    b: &std::collections::HashMap<String, Value>,
    keys: &BTreeSet<String>,
) -> Vec<TableRow> {
    let mut rows = Vec::new();
    for k in keys {
        let ca = cu(a.get(k));
        let cb = cu(b.get(k));
        let (delta_str, pct_str, kind) = match (ca, cb) {
            (Some(ca), Some(cb)) => {
                let delta = cb as i128 - ca as i128;
                let pct = if ca == 0 {
                    "n/a (baseline 0)".to_string()
                } else {
                    format!("{:+.2}%", 100.0 * (cb as f64 - ca as f64) / ca as f64)
                };
                let kind = match delta.cmp(&0) {
                    std::cmp::Ordering::Less => DeltaKind::Improved,
                    std::cmp::Ordering::Greater => DeltaKind::Worse,
                    std::cmp::Ordering::Equal => DeltaKind::Unchanged,
                };
                (format_delta_i128(delta), pct, kind)
            }
            (None, Some(_)) => ("—".to_string(), "new key".to_string(), DeltaKind::NewKey),
            (Some(_), None) => ("—".to_string(), "removed".to_string(), DeltaKind::Removed),
            (None, None) => ("—".to_string(), "—".to_string(), DeltaKind::Missing),
        };
        rows.push(TableRow {
            key: k.clone(),
            baseline: fmt_u64(ca),
            current: fmt_u64(cb),
            delta: delta_str,
            pct: pct_str,
            kind,
        });
    }
    rows
}

mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const CYAN: &str = "\x1b[36m";
    pub const WHITE: &str = "\x1b[97m";
}

fn style_cell(s: String, color: &str, use_color: bool) -> String {
    if !use_color {
        return s;
    }
    format!("{color}{s}{}", ansi::RESET)
}

fn color_for_delta(kind: DeltaKind) -> &'static str {
    match kind {
        DeltaKind::Improved => ansi::GREEN,
        DeltaKind::Worse => ansi::RED,
        DeltaKind::Unchanged => ansi::DIM,
        DeltaKind::NewKey => ansi::YELLOW,
        DeltaKind::Removed => ansi::YELLOW,
        DeltaKind::Missing => ansi::DIM,
    }
}

fn print_terminal_table(
    title: &str,
    baseline_path: &Path,
    current_path: &Path,
    rows: &[TableRow],
    use_color: bool,
) {
    const KEY_MAX: usize = 56;
    let hdr_key = "Key";
    let hdr_b = "Baseline CU";
    let hdr_c = "Current CU";
    let hdr_d = "Δ CU";
    let hdr_p = "Δ %";

    let mut w_key = display_width(hdr_key);
    let mut w_b = display_width(hdr_b);
    let mut w_c = display_width(hdr_c);
    let mut w_d = display_width(hdr_d);
    let mut w_p = display_width(hdr_p);

    for row in rows {
        w_key = w_key.max(display_width(&row.key).min(KEY_MAX));
        w_b = w_b.max(display_width(&row.baseline));
        w_c = w_c.max(display_width(&row.current));
        w_d = w_d.max(display_width(&row.delta));
        w_p = w_p.max(display_width(&row.pct));
    }
    w_key = w_key.clamp(3, KEY_MAX);

    let sep_width = w_key + w_b + w_c + w_d + w_p + 4;

    let title_line = if use_color {
        format!("{}{}{}{}", ansi::BOLD, ansi::CYAN, title, ansi::RESET)
    } else {
        title.to_string()
    };
    println!("{title_line}");
    println!();

    let base_line = if use_color {
        format!(
            "{}{}Baseline:{} {}",
            ansi::BOLD,
            ansi::WHITE,
            ansi::RESET,
            baseline_path.display()
        )
    } else {
        format!("Baseline: {}", baseline_path.display())
    };
    let cur_line = if use_color {
        format!(
            "{}{}Current:{} {}",
            ansi::BOLD,
            ansi::WHITE,
            ansi::RESET,
            current_path.display()
        )
    } else {
        format!("Current: {}", current_path.display())
    };
    println!("{base_line}");
    println!("{cur_line}");
    println!();

    let sep: String = "─".repeat(sep_width);
    println!(
        "{}",
        if use_color {
            format!("{}{}{}", ansi::DIM, sep, ansi::RESET)
        } else {
            sep
        }
    );

    let head = format!(
        "{} {} {} {} {}",
        pad_right(hdr_key, w_key),
        pad_left(hdr_b, w_b),
        pad_left(hdr_c, w_c),
        pad_left(hdr_d, w_d),
        pad_left(hdr_p, w_p)
    );
    println!(
        "{}",
        if use_color {
            format!("{}{}{}", ansi::BOLD, head, ansi::RESET)
        } else {
            head
        }
    );

    let sep2: String = "─".repeat(sep_width);
    println!(
        "{}",
        if use_color {
            format!("{}{}{}", ansi::DIM, sep2, ansi::RESET)
        } else {
            sep2
        }
    );

    for row in rows {
        let key_disp = truncate_chars(&row.key, w_key);
        let key_cell = pad_right(&key_disp, w_key);
        let b_cell = pad_left(&row.baseline, w_b);
        let c_cell = pad_left(&row.current, w_c);
        let d_cell = style_cell(
            pad_left(&row.delta, w_d),
            color_for_delta(row.kind),
            use_color,
        );
        let p_cell = style_cell(
            pad_left(&row.pct, w_p),
            color_for_delta(row.kind),
            use_color,
        );

        println!("{key_cell} {b_cell} {c_cell} {d_cell} {p_cell}");
    }

    let sep3: String = "─".repeat(sep_width);
    println!(
        "{}",
        if use_color {
            format!("{}{}{}", ansi::DIM, sep3, ansi::RESET)
        } else {
            sep3
        }
    );
    println!();
}

fn main() {
    let args = parse_args();
    let a = load_object_map(&args.baseline);
    let b = load_object_map(&args.current);

    let keys: BTreeSet<_> = a.keys().chain(b.keys()).cloned().collect();
    let rows = build_rows(&a, &b, &keys);

    let stdout = io::stdout();
    let use_color = stdout.is_terminal() && !args.force_no_color && !no_color_requested();

    print_terminal_table(&args.title, &args.baseline, &args.current, &rows, use_color);
}
