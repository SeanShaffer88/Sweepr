mod cli;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    if let Some(code) = cli::run(&args, &mut out, &mut err) {
        std::process::exit(code);
    }
    eprintln!("sweepr: GUI not built yet");
    std::process::exit(2);
}
