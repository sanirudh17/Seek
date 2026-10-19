mod install;
mod server;
mod service;
use seek_engine::{helper, kernel::Cancel};
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let option = |key: &str| args.windows(2).find(|w| w[0] == key).map(|w| w[1].clone());
    let sid = option("--sid").unwrap_or(helper::user_sid()?);
    helper::validate_sid(&sid)?;
    match args.first().map(String::as_str) {
        Some("install") => install::install(&sid)?,
        Some("uninstall") => install::uninstall(&sid)?,
        Some("service") => service::dispatch(sid)?,
        Some("serve") => service::serve(&sid, Cancel::new()?)?,
        Some("status") => println!("user={sid} service_pid={:?}", helper::service_pid(&sid)?),
        _ => println!(
            "seek-helper install|uninstall|status|serve --sid USER_SID\nInstallation/uninstallation require an administrator. Seek itself stays unelevated."
        ),
    }
    Ok(())
}
