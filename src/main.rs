//! The LimeDAW binary.
//!
//! Running it with no arguments opens the interface. `--mcp-stdio` instead
//! bridges stdin/stdout onto the running app's unix socket, which is how MCP
//! clients that spawn a process connect.

fn main() -> iced::Result {
    #[cfg(not(target_arch = "wasm32"))]
    {
        env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("limedaw=info,warn"),
        )
        .init();

        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.first().map(String::as_str) {
            Some("--mcp-stdio") => {
                if let Err(err) = limedaw::mcp::stdio_bridge() {
                    eprintln!("limedaw: MCP bridge failed: {err}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            Some("--version" | "-V") => {
                println!("limedaw {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--help" | "-h") => {
                print_help();
                return Ok(());
            }
            Some("--paths") => {
                print_paths();
                return Ok(());
            }
            _ => {}
        }
    }

    #[cfg(target_os = "linux")]
    configure_fontconfig();

    limedaw::run()
}

#[cfg(target_os = "linux")]
fn configure_fontconfig() {
    use std::env;

    // iced's font loader reads FONTCONFIG_FILE but skips FONTCONFIG_PATH,
    // which Guix uses to locate its system font configuration.
    if env::var_os("FONTCONFIG_FILE").is_some() {
        return;
    }
    let Some(paths) = env::var_os("FONTCONFIG_PATH") else {
        return;
    };
    if let Some(config) = env::split_paths(&paths)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join("fonts.conf"))
        .find(|config| config.is_file())
    {
        // SAFETY: this runs in main before LimeDAW starts any threads.
        unsafe { env::set_var("FONTCONFIG_FILE", config) };
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn print_help() {
    println!(
        "LimeDAW {version}

USAGE:
    limedaw                 Open the digital audio workstation
    limedaw --mcp-stdio     Bridge stdin/stdout to the running app's MCP socket
    limedaw --paths         Show where LimeDAW keeps its files
    limedaw --version       Print the version

ENVIRONMENT:
    LIMEDAW_MCP_SOCKET      Override the MCP socket path
    ICED_BACKEND            Force a renderer: wgpu (GPU) or tiny-skia (software)
",
        version = env!("CARGO_PKG_VERSION")
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn print_paths() {
    println!("projects   {}", limedaw::paths::projects_dir().display());
    println!("renders    {}", limedaw::paths::renders_dir().display());
    println!("samples    {}", limedaw::paths::samples_dir().display());
    println!("settings   {}", limedaw::paths::settings_file().display());
    println!("data       {}", limedaw::paths::data_dir().display());
    println!(
        "mcp socket {}",
        limedaw::mcp::server::socket_path().display()
    );
}
