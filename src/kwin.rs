use std::error::Error;
use std::path::PathBuf;

use crate::app::PartyConfig;
use crate::instance::{instance_layout_regions, Instance};
use crate::monitor::Monitor;
use crate::paths::PATH_PARTY;

pub fn kwin_dbus_start_script(file: PathBuf) -> Result<(), Box<dyn Error>> {
    println!(
        "[partydeck] kwin::kwin_dbus_start_script - Loading script {}...",
        file.display()
    );
    if !file.exists() {
        return Err("[partydeck] kwin::kwin_dbus_start_script - Script file doesn't exist!".into());
    }

    let conn = zbus::blocking::Connection::session()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )?;

    let _: i32 = proxy.call("loadScript", &(file.to_string_lossy(), "splitscreen"))?;
    println!("[partydeck] kwin::kwin_dbus_start_script - Script loaded. Starting...");
    let _: () = proxy.call("start", &())?;

    println!("[partydeck] kwin::kwin_dbus_start_script - KWin script started.");
    Ok(())
}

pub fn kwin_dbus_unload_script() -> Result<(), Box<dyn Error>> {
    println!("[partydeck] kwin::kwin_dbus_unload_script - Unloading splitscreen script...");
    let conn = zbus::blocking::Connection::session()?;
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
    )?;

    let _: bool = proxy.call("unloadScript", &("splitscreen"))?;

    println!("[partydeck] kwin::kwin_dbus_unload_script - Script unloaded.");
    Ok(())
}

pub fn write_kwin_layout_script(
    instances: &[Instance],
    monitors: &[Monitor],
    cfg: &PartyConfig,
    layout_rotation: u8,
    pids: &[u32],
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let tmp_dir = PATH_PARTY.join("tmp");
    std::fs::create_dir_all(&tmp_dir)?;

    let path = tmp_dir.join("splitscreen_kwin.js");
    std::fs::write(&path, build_kwin_layout_script(instances, monitors, cfg, layout_rotation, pids))?;

    Ok(path)
}

fn build_kwin_layout_script(instances: &[Instance], monitors: &[Monitor], cfg: &PartyConfig, layout_rotation: u8, pids: &[u32]) -> String {
    let pid_list = pids
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let regions = instance_layout_regions(instances, cfg.vertical_two_player, layout_rotation);
    let layout = regions
        .iter()
        .zip(pids.iter())
        .map(|(rect, pid)| {
            format!(
                r#"  {}: {{ x: {}, y: {}, width: {}, height: {} }}"#,
                pid,
                rect.x,
                rect.y,
                rect.w,
                rect.h
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");

    let monitor_map = instances
        .iter()
        .zip(pids.iter())
        .map(|(inst, pid)| {
            let name = monitors
                .get(inst.monitor)
                .map(|m| m.name())
                .unwrap_or("");
            format!(r#"  {}: "{}""#, pid, name)
        })
        .collect::<Vec<_>>()
        .join(",\n");

    format!(
        r#"var pids = [{pid_list}];
var layout = {{
{layout}
}};
var monitorNames = {{
{monitor_map}
}};

function clientArea(client) {{
  if (!client || !client.frameGeometry) {{
    return 0;
  }}
  return client.frameGeometry.width * client.frameGeometry.height;
}}

function isManagedClient(client) {{
  return !!(client && client.pid && layout[client.pid]);
}}

function shouldPreferClient(candidate, current) {{
  if (!current) {{
    return true;
  }}

  var candidateNormal = !!candidate.normalWindow;
  var currentNormal = !!current.normalWindow;
  if (candidateNormal != currentNormal) {{
    return candidateNormal;
  }}

  return clientArea(candidate) > clientArea(current);
}}

function getManagedClients() {{
  var allClients = workspace.windowList();
  var managedByPid = {{}};

  for (var i = 0; i < allClients.length; i++) {{
    var client = allClients[i];
    if (!isManagedClient(client)) {{
      continue;
    }}

    var p = client.pid;
    if (shouldPreferClient(client, managedByPid[p])) {{
      managedByPid[p] = client;
    }}
  }}

  var managedClients = [];
  for (var pidIndex = 0; pidIndex < pids.length; pidIndex++) {{
    var managedClient = managedByPid[pids[pidIndex]];
    if (managedClient) {{
      managedClients.push(managedClient);
    }}
  }}

  return managedClients;
}}

function findOutputByName(name) {{
  try {{
    var screens = workspace.screens;
    for (var i = 0; i < screens.length; i++) {{
      if (screens[i].name === name) {{
        return screens[i];
      }}
    }}
  }} catch (e) {{}}
  return null;
}}

function applyLayout(client) {{
  var spec = layout[client.pid];
  if (!spec) {{
    return;
  }}

  var targetName = monitorNames[client.pid];
  var targetOutput = null;
  if (targetName) {{
    targetOutput = findOutputByName(targetName);
  }}
  if (targetOutput && (!client.output || client.output.name !== targetOutput.name)) {{
    try {{
      workspace.sendClientToScreen(client, targetOutput);
    }} catch (e) {{}}
  }}
  var monitor = targetOutput || client.output;
  if (!monitor) {{
    monitor = client.output;
  }}
  if (!monitor || !monitor.geometry) {{
    return;
  }}

  var monitorX = monitor.geometry.x;
  var monitorY = monitor.geometry.y;
  var monitorWidth = monitor.geometry.width;
  var monitorHeight = monitor.geometry.height;

  var targetGeometry = {{
    x: Math.round(monitorX + spec.x * monitorWidth),
    y: Math.round(monitorY + spec.y * monitorHeight),
    width: Math.round(monitorWidth * spec.width),
    height: Math.round(monitorHeight * spec.height),
  }};

  client.noBorder = true;
  if (
    !client.frameGeometry ||
    client.frameGeometry.x !== targetGeometry.x ||
    client.frameGeometry.y !== targetGeometry.y ||
    client.frameGeometry.width !== targetGeometry.width ||
    client.frameGeometry.height !== targetGeometry.height
  ) {{
    client.frameGeometry = targetGeometry;
  }}
}}

function gamescopeAboveBelow() {{
  var managedClients = getManagedClients();
  var activeWindow = workspace.activeWindow;
  var keepAbove = isManagedClient(activeWindow);

  for (var i = 0; i < managedClients.length; i++) {{
    managedClients[i].keepAbove = keepAbove;
  }}
}}

function gamescopeSplitscreen() {{
  var managedClients = getManagedClients();
  for (var i = 0; i < managedClients.length; i++) {{
    applyLayout(managedClients[i]);
  }}
  gamescopeAboveBelow();
}}

workspace.windowAdded.connect(function(client) {{
  gamescopeSplitscreen();
}});
workspace.windowRemoved.connect(gamescopeSplitscreen);
workspace.windowActivated.connect(gamescopeAboveBelow);
gamescopeSplitscreen();
"#
    )
}
