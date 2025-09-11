use crate::monitor::Monitor;
use dialog::{Choice, DialogBox};
use std::error::Error;
use std::path::PathBuf;

pub fn msg(title: &str, contents: &str) {
    let _ = dialog::Message::new(contents).title(title).show();
}

pub fn yesno(title: &str, contents: &str) -> bool {
    if let Ok(prompt) = dialog::Question::new(contents).title(title).show() {
        if prompt == Choice::Yes {
            return true;
        }
    }
    false
}

// Sends the splitscreen script to the active KWin session through DBus
pub fn kwin_dbus_start_script(file: PathBuf) -> Result<(), Box<dyn Error>> {
    println!(
        "[partydeck] util::kwin_dbus_start_script - Loading script {}...",
        file.display()
    );
    if !file.exists() {
        return Err("[partydeck] util::kwin_dbus_start_script - Script file doesn't exist!".into());
    }

    let conn = zbus::blocking::Connection::session()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )?;

    let _: i32 = proxy.call("loadScript", &(file.to_string_lossy(), "splitscreen"))?;
    println!("[partydeck] util::kwin_dbus_start_script - Script loaded. Starting...");
    let _: () = proxy.call("start", &())?;

    println!("[partydeck] util::kwin_dbus_start_script - KWin script started.");
    Ok(())
}

pub fn kwin_dbus_unload_script() -> Result<(), Box<dyn Error>> {
    println!("[partydeck] util::kwin_dbus_unload_script - Unloading splitscreen script...");
    let conn = zbus::blocking::Connection::session()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )?;

    let _: bool = proxy.call("unloadScript", &("splitscreen"))?;

    println!("[partydeck] util::kwin_dbus_unload_script - Script unloaded.");
    Ok(())
}

pub fn launch_kwin_session(monitors: &[Monitor]) {
    let (w, h) = (monitors[0].width(), monitors[0].height());
    let mut cmd = std::process::Command::new("kwin_wayland");

    cmd.arg("--xwayland");
    cmd.arg("--width");
    cmd.arg(w.to_string());
    cmd.arg("--height");
    cmd.arg(h.to_string());
    cmd.arg("--exit-with-session");
    
    let args: Vec<String> = std::env::args()
        .filter(|arg| arg != "--kwin")
        .collect();
    let args_string = args
        .iter()
        .map(|arg| format!("\"{}\"", arg))
        .collect::<Vec<String>>()
        .join(" ");
    cmd.arg(args_string);

    println!("[partydeck] Launching kwin session: {:?}", cmd);

    match cmd.spawn() {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            eprintln!("[partydeck] Failed to start kwin_wayland: {}", e);
            std::process::exit(1);
        }
    }
}