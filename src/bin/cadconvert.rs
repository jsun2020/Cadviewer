use std::path::PathBuf;
use std::process::ExitCode;

use cadviewer::converter::{ConvertOptions, convert_to_pdf};
use cadviewer::plot::style::ColorMode;

const USAGE: &str =
    "用法：Cadconvert.exe <input.dwg|dxf> <output.pdf> [--mono] [--all|--sheet N] [--font-dir <path>]";

/// R-CLI exit codes: 1 input error, 2 decode failure, 3 nothing to print.
const EXIT_INPUT: u8 = 1;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(input) = args.next() else {
        eprintln!("{USAGE}");
        return ExitCode::from(EXIT_INPUT);
    };
    let Some(output) = args.next() else {
        eprintln!("{USAGE}");
        return ExitCode::from(EXIT_INPUT);
    };

    let rest: Vec<String> = args.map(|a| a.to_string_lossy().into_owned()).collect();
    let options = match parse_options(&rest) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("错误：{message}\n{USAGE}");
            return ExitCode::from(EXIT_INPUT);
        }
    };

    match convert_to_pdf(&PathBuf::from(input), &PathBuf::from(output), &options) {
        Ok((pages, warnings)) => {
            println!("已导出 {pages} 页");
            // R-TXT-2.3: a substituted font must never be invisible from the
            // command line — the page still exports, but not in the font the
            // drawing asked for.
            for warning in &warnings {
                eprintln!("警告：{warning}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("错误：{error}");
            ExitCode::from(error.exit_code())
        }
    }
}

/// Parse the option tail, rejecting what it cannot honour.
///
/// A malformed `--sheet` used to be swallowed: `--sheet abc` parsed to
/// `None`, which means "export everything", so asking for one page and
/// getting all 26 looked like success. Unknown flags were ignored for the
/// same reason.
fn parse_options(args: &[String]) -> Result<ConvertOptions, String> {
    let mut options = ConvertOptions::default();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--mono" => options.mode = ColorMode::Monochrome,
            "--all" => options.sheet = None,
            "--sheet" => {
                let value = args.get(i + 1).ok_or("--sheet 需要一个页码")?;
                let index: usize = value
                    .parse()
                    .map_err(|_| format!("--sheet 需要一个正整数页码，收到 {value:?}"))?;
                if index == 0 {
                    return Err("图纸编号从 1 开始".to_owned());
                }
                options.sheet = Some(index);
                i += 1;
            }
            "--font-dir" => {
                let value = args.get(i + 1).ok_or("--font-dir 需要一个目录")?;
                options.font_dirs.push(PathBuf::from(value));
                i += 1;
            }
            other => return Err(format!("未知选项 {other:?}")),
        }
        i += 1;
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn no_options_exports_every_sheet_in_colour() {
        let options = parse_options(&[]).unwrap();
        assert_eq!(options.sheet, None);
        assert_eq!(options.mode, ColorMode::Color);
    }

    #[test]
    fn flags_are_honoured() {
        let options = parse_options(&args(&["--mono", "--sheet", "7"])).unwrap();
        assert_eq!(options.sheet, Some(7));
        assert_eq!(options.mode, ColorMode::Monochrome);
    }

    /// A malformed page number must be an error, not a silent
    /// "export all 26 pages" when the caller asked for one.
    #[test]
    fn a_non_numeric_sheet_is_rejected() {
        assert!(parse_options(&args(&["--sheet", "abc"])).is_err());
        assert!(parse_options(&args(&["--sheet"])).is_err());
        assert!(parse_options(&args(&["--sheet", "0"])).is_err());
    }

    #[test]
    fn unknown_flags_are_rejected_rather_than_ignored() {
        assert!(parse_options(&args(&["--colour"])).is_err());
    }

    /// R-CLI: `--font-dir` appends a search directory (R-TXT-2.1 step 4).
    #[test]
    fn font_dir_is_collected() {
        let options = parse_options(&args(&["--font-dir", "C:\\fonts"])).unwrap();
        assert_eq!(options.font_dirs, vec![PathBuf::from("C:\\fonts")]);
    }

    #[test]
    fn font_dir_may_be_repeated() {
        let options = parse_options(&args(&["--font-dir", "A", "--font-dir", "B"])).unwrap();
        assert_eq!(options.font_dirs.len(), 2);
    }

    /// A flag that cannot be honoured must be an error, not ignored — the
    /// defect fixed in commit 12ea4bd.
    #[test]
    fn font_dir_without_a_value_is_rejected() {
        assert!(parse_options(&args(&["--font-dir"])).is_err());
    }
}
