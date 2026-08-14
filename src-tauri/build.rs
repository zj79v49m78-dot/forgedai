//! Build script.
//!
//! Embeds an application manifest requesting `requireAdministrator`. Forged
//! writes to HKLM, reconfigures services and calls powercfg, none of which work
//! from a standard-user token — so it asks for elevation at launch rather than
//! failing halfway through a run with half the plan applied.
//!
//! `uiAccess="false"` and the explicit `longPathAware` and DPI entries keep the
//! manifest honest: elevation is requested for the operations that need it, not
//! as a blanket grab.
//!
//! ## Do not remove the Common-Controls dependency
//!
//! Supplying a custom manifest *replaces* the default one Tauri would otherwise
//! embed — it does not merge with it. The `<dependency>` block below is part of
//! that default, and dropping it is not cosmetic: without an explicit dependency
//! on ComCtl32 version 6, Windows loads version 5, which does not export
//! `TaskDialogIndirect`. The app then dies at launch with
//!
//!     The procedure entry point TaskDialogIndirect could not be located
//!     in the dynamic link library ...
//!
//! before any of our code runs, so nothing in the app can catch or report it.
//! This exact bug shipped in the first build. Keep the block.

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="com.forged.optimiser" version="1.0.0.0" />
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v2">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 and 11 -->
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}" />
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
</assembly>
"#;

/// Fails the build rather than shipping an executable that cannot start.
///
/// The Common-Controls omission could not be caught by any test we run: the
/// binary compiles, links, and passes CI, then dies in the Windows loader before
/// `main` on the user's machine. A string check at build time is crude, but it
/// is the only place this class of mistake can be caught at all.
fn assert_manifest_is_complete() {
    for (needle, why) in [
        (
            "Microsoft.Windows.Common-Controls",
            "without it Windows loads ComCtl32 v5, which lacks TaskDialogIndirect, \
             and the app dies in the loader before main",
        ),
        (
            "requireAdministrator",
            "Forged writes to HKLM and reconfigures services; without elevation every \
             actuator fails",
        ),
    ] {
        assert!(
            MANIFEST.contains(needle),
            "application manifest is missing `{needle}` — {why}"
        );
    }
}

fn main() {
    assert_manifest_is_complete();

    let windows = tauri_build::WindowsAttributes::new().app_manifest(MANIFEST);

    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run tauri-build");
}
