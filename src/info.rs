pub const ABOUT: &str = concat!(
    "MaxonKey ",
    env!("CARGO_PKG_VERSION"),
    "\nMaxon Torres Software\n\n",
    "Personal, local-first secrets manager.\n",
    "Windows first; Linux and macOS later.\n\n",
    "Foundation prototype: CLI and TUI only.\n",
    "No secrets are read or stored."
);

pub const UNAVAILABLE: [&str; 8] = [
    "Encrypted vault",
    "Encryption",
    "SQLite storage",
    "Secret CRUD",
    "Projects / environments",
    "MCP server",
    "MCP owner approval",
    "Secure sharing",
];

pub fn status() -> String {
    let mut output = String::from("MaxonKey foundation prototype\nCLI / TUI: IMPLEMENTED\n");
    for label in UNAVAILABLE {
        output.push_str(&format!("{label}: NOT IMPLEMENTED\n"));
    }
    output.push_str("No secrets are read or stored.\n");
    output
}
