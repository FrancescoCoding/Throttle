//! Launch at Windows startup via a Task Scheduler logon task.
//!
//! The usual `HKCU\...\Run` key does not work for Throttle: the release build's
//! manifest requires elevation, and Windows silently skips Run entries that
//! need it. A scheduled task with "run with highest privileges" starts elevated
//! at logon without a UAC prompt. Throttle is already elevated when it creates
//! the task, so no extra prompt is needed for that either.
//!
//! The task's existence is the source of truth for the setting; nothing is
//! stored in `settings.json`.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};

/// Task Scheduler name of the logon task.
const TASK_NAME: &str = "Throttle";

/// Command-line flag the task passes so a logon start goes straight to the tray.
pub const TRAY_FLAG: &str = "--tray";

/// Keep `schtasks` from flashing a console window (the release build has none).
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn schtasks(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

/// Whether the logon task exists.
pub fn is_enabled() -> bool {
    schtasks(&["/Query", "/TN", TASK_NAME]).is_ok_and(|out| out.status.success())
}

/// Create (or replace) the logon task so it starts `exe` in the tray.
pub fn enable(exe: &Path) -> anyhow::Result<()> {
    let user = current_user();
    let xml = task_xml(exe, &user);

    // schtasks reads task XML as UTF-16 with a BOM.
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    let xml_path = std::env::temp_dir().join("throttle-autostart.xml");
    std::fs::write(&xml_path, bytes).context("write task XML")?;

    let xml_arg = xml_path.to_string_lossy();
    let out = schtasks(&["/Create", "/TN", TASK_NAME, "/XML", &xml_arg, "/F"]);
    let _ = std::fs::remove_file(&xml_path);
    let out = out.context("run schtasks")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Delete the logon task.
pub fn disable() -> anyhow::Result<()> {
    let out = schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]).context("run schtasks")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// `DOMAIN\user` for the logon trigger, so the task fires for this user only.
fn current_user() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    }
}

/// Task definition. The non-default settings matter:
///  - `ExecutionTimeLimit PT0S`: the default (72 hours) would kill Throttle;
///  - both battery flags off: the defaults skip or stop the task on battery;
///  - `Priority 4`: the default (7) runs the shaper below normal priority.
fn task_xml(exe: &Path, user: &str) -> String {
    let command = xml_escape(&exe.to_string_lossy());
    let workdir = xml_escape(&exe.parent().unwrap_or(Path::new("")).to_string_lossy());
    let user = xml_escape(user);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Start Throttle in the tray when you sign in.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{TRAY_FLAG}</Arguments>
      <WorkingDirectory>{workdir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_xml_escapes_paths_and_user() {
        let xml = task_xml(
            Path::new(r"C:\Tools & Apps\Throttle\throttle.exe"),
            r"PC\O'Brien",
        );
        assert!(xml.contains(r"<Command>C:\Tools &amp; Apps\Throttle\throttle.exe</Command>"));
        assert!(xml.contains(r"<WorkingDirectory>C:\Tools &amp; Apps\Throttle</WorkingDirectory>"));
        assert!(xml.contains(r"<UserId>PC\O&apos;Brien</UserId>"));
    }

    #[test]
    fn task_xml_never_stops_or_deprioritises_the_shaper() {
        let xml = task_xml(Path::new(r"C:\throttle.exe"), "me");
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
        assert!(xml.contains("<Priority>4</Priority>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains(&format!("<Arguments>{TRAY_FLAG}</Arguments>")));
    }
}
