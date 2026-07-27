use anyhow::Result;

fn main() {
    let exit_code = match try_main() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error:#}");
            1
        }
    };
    std::process::exit(exit_code);
}

fn try_main() -> Result<i32> {
    let cli = cx::cli::Cli::parse_cx();
    cx::dispatch::execute(&cli)
}
