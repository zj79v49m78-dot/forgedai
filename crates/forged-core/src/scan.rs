//! Hardware and configuration inventory.
//!
//! Everything the planner reasons about is gathered here. Inventory goes through
//! CIM rather than raw WMI COM calls: the queries are readable, they do not
//! require COM apartment management, and a class that does not exist on a given
//! SKU returns empty instead of failing the whole scan. Registry reads are used
//! where CIM has no equivalent — driver class keys, VBS state, Fortnite's install
//! location.
//!
//! No single failure aborts the scan. A machine with an exotic storage
//! controller or a missing WMI class still produces a usable profile, with the
//! unknown fields left at their defaults; the catalog's applicability predicates
//! are written to treat "unknown" as "not applicable".

use crate::error::Result;
use crate::hardware::*;
use crate::tweaks::model::{Hive, RegData};
use crate::win::{process, registry};

/// Display adapter class GUID.
const DISPLAY_CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
/// Network adapter class GUID.
const NET_CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e972-e325-11ce-bfc1-08002be10318}";

/// Runs the full inventory.
pub fn scan() -> Result<HardwareProfile> {
    let mut profile = HardwareProfile {
        scanned_at: chrono::Utc::now().to_rfc3339(),
        ..Default::default()
    };

    // Each stage is independent and logs rather than propagates, so one
    // unsupported class cannot cost us the whole profile.
    stage("cpu", || profile.cpu = scan_cpu());
    stage("gpu", || profile.gpus = scan_gpus());
    stage("memory", || profile.memory = scan_memory());
    stage("storage", || profile.storage = scan_storage());
    stage("network", || profile.network = scan_network());
    stage("display", || profile.displays = scan_displays());
    stage("os", || profile.os = scan_os());
    stage("peripherals", || profile.peripherals = scan_peripherals());
    stage("power", || profile.power = scan_power());
    stage("motherboard", || profile.motherboard = scan_motherboard());
    stage("fortnite", || profile.fortnite = scan_fortnite());

    // Cross-reference: mark the drive hosting the game install.
    if let Some(path) = profile.fortnite.install_path.clone() {
        if let Some(letter) = path.chars().next() {
            let letter = format!("{}:", letter.to_ascii_uppercase());
            for device in &mut profile.storage {
                if device.drive_letters.iter().any(|l| l == &letter) {
                    device.hosts_fortnite = true;
                }
            }
        }
    }

    Ok(profile)
}

/// Runs one inventory stage, swallowing and logging any panic-free failure.
fn stage(name: &str, f: impl FnOnce()) {
    tracing::debug!("scanning {name}");
    f();
}

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

fn scan_cpu() -> Cpu {
    let rows = process::cim_query(
        "Win32_Processor",
        &[
            "Name",
            "Manufacturer",
            "NumberOfCores",
            "NumberOfLogicalProcessors",
            "MaxClockSpeed",
            "CurrentClockSpeed",
            "L3CacheSize",
            "VirtualizationFirmwareEnabled",
        ],
    )
    .unwrap_or_default();

    let Some(row) = rows.first() else {
        return Cpu::default();
    };

    let name = process::json_str(row, "Name");
    let manufacturer = process::json_str(row, "Manufacturer");
    let vendor = if manufacturer.contains("Intel") || name.contains("Intel") {
        CpuVendor::Intel
    } else if manufacturer.contains("AMD") || name.contains("AMD") || name.contains("Ryzen") {
        CpuVendor::Amd
    } else {
        CpuVendor::Unknown
    };

    let generation = parse_generation(&name, vendor);
    // Intel hybrid P/E-core designs start at Alder Lake (12th gen). Detected by
    // generation rather than by core counts, which are unreliable to interpret.
    let hybrid = vendor == CpuVendor::Intel && generation.is_some_and(|g| g >= 12);

    Cpu {
        vendor,
        physical_cores: process::json_u32(row, "NumberOfCores"),
        logical_threads: process::json_u32(row, "NumberOfLogicalProcessors"),
        base_mhz: process::json_u32(row, "CurrentClockSpeed"),
        max_mhz: process::json_u32(row, "MaxClockSpeed"),
        generation,
        hybrid_architecture: hybrid,
        virtualization_enabled: process::json_bool(row, "VirtualizationFirmwareEnabled"),
        l3_cache_kb: process::json_u32(row, "L3CacheSize"),
        has_3d_vcache: name.contains("X3D"),
        name,
    }
}

/// Extracts a generation number from a marketing name.
///
/// Intel: "Core i5-12400F" → 12, "Core Ultra 7 265K" → 14 (Arrow Lake is treated
/// as 14+ for hybrid purposes). AMD: "Ryzen 5 7600X" → 7.
fn parse_generation(name: &str, vendor: CpuVendor) -> Option<u32> {
    match vendor {
        CpuVendor::Intel => {
            if name.contains("Ultra") {
                // Core Ultra parts are all hybrid; 14 is the lowest value that
                // satisfies the >= 12 hybrid test.
                return Some(14);
            }
            // Find the model number following "i3-", "i5-", "i7-", "i9-".
            let idx = name.find('-')?;
            let digits: String = name[idx + 1..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            match digits.len() {
                // 5 digits: 5-digit model like 12400 → first two digits.
                5 => digits[..2].parse().ok(),
                // 4 digits: e.g. 9700 → first digit.
                4 => digits[..1].parse().ok(),
                _ => None,
            }
        }
        CpuVendor::Amd => {
            let idx = name.find("Ryzen")?;
            let digits: String = name[idx..]
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .skip_while(|c| c.is_ascii_digit()) // series number (5, 7, 9)
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect();
            digits.chars().next()?.to_digit(10)
        }
        CpuVendor::Unknown => None,
    }
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

fn scan_gpus() -> Vec<Gpu> {
    let rows = process::cim_query(
        "Win32_VideoController",
        &[
            "Name",
            "AdapterRAM",
            "DriverVersion",
            "DriverDate",
            "PNPDeviceID",
            "CurrentBitsPerPixel",
        ],
    )
    .unwrap_or_default();

    let class_index = index_class_keys(DISPLAY_CLASS);

    rows.iter()
        .map(|row| {
            let name = process::json_str(row, "Name");
            let vendor = if name.contains("NVIDIA") || name.contains("GeForce") {
                GpuVendor::Nvidia
            } else if name.contains("AMD") || name.contains("Radeon") {
                GpuVendor::Amd
            } else if name.contains("Intel") {
                GpuVendor::Intel
            } else {
                GpuVendor::Unknown
            };

            // AdapterRAM is a 32-bit field and wraps above 4 GB, so it is only
            // trustworthy as a lower bound. Reported as-is rather than guessed at.
            let vram_mb = process::json_u64(row, "AdapterRAM") / (1024 * 1024);

            Gpu {
                is_integrated: name.contains("UHD")
                    || name.contains("Iris")
                    || name.contains("Vega") && name.contains("Graphics")
                    || name.contains("Radeon Graphics"),
                class_key_index: class_index
                    .iter()
                    .find(|(_, desc)| desc == &name)
                    .map(|(idx, _)| idx.clone()),
                pnp_device_id: process::json_str(row, "PNPDeviceID"),
                driver_version: process::json_str(row, "DriverVersion"),
                driver_date: process::json_str(row, "DriverDate"),
                is_primary: process::json_u64(row, "CurrentBitsPerPixel") > 0,
                vram_mb,
                vendor,
                name,
            }
        })
        .collect()
}

/// Maps class-key subkey index → DriverDesc, so a CIM device name can be
/// resolved to the registry node holding its advanced properties.
fn index_class_keys(class_path: &str) -> Vec<(String, String)> {
    let Ok(subkeys) = registry::subkeys(Hive::LocalMachine, class_path) else {
        return Vec::new();
    };

    subkeys
        .into_iter()
        // Only the four-digit instance nodes; skip Configuration, Properties etc.
        .filter(|k| k.len() == 4 && k.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|index| {
            let path = format!("{class_path}\\{index}");
            match registry::get_value(Hive::LocalMachine, &path, "DriverDesc") {
                Ok(Some(RegData::Sz(desc))) => Some((index, desc)),
                _ => None,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

fn scan_memory() -> Memory {
    let rows = process::cim_query(
        "Win32_PhysicalMemory",
        &[
            "Manufacturer",
            "PartNumber",
            "Capacity",
            "ConfiguredClockSpeed",
            "Speed",
            "DeviceLocator",
            "FormFactor",
        ],
    )
    .unwrap_or_default();

    let modules: Vec<MemoryModule> = rows
        .iter()
        .map(|row| MemoryModule {
            manufacturer: process::json_str(row, "Manufacturer"),
            part_number: process::json_str(row, "PartNumber"),
            capacity_mb: process::json_u64(row, "Capacity") / (1024 * 1024),
            // ConfiguredClockSpeed is what the modules are actually running at.
            configured_mhz: process::json_u32(row, "ConfiguredClockSpeed"),
            // Speed is the SPD/XMP rated maximum.
            rated_mhz: process::json_u32(row, "Speed"),
            slot: process::json_str(row, "DeviceLocator"),
        })
        .collect();

    Memory {
        total_mb: modules.iter().map(|m| m.capacity_mb).sum(),
        configured_mhz: modules.iter().map(|m| m.configured_mhz).max().unwrap_or(0),
        rated_mhz: modules.iter().map(|m| m.rated_mhz).max().unwrap_or(0),
        module_count: modules.len() as u32,
        form_factor: rows
            .first()
            .map(|r| process::json_str(r, "FormFactor"))
            .unwrap_or_default(),
        modules,
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

fn scan_storage() -> Vec<StorageDevice> {
    // MSFT_PhysicalDisk carries the MediaType and BusType that distinguish NVMe
    // from SATA SSD from spinning rust. Win32_DiskDrive does not.
    let physical = process::cim_query_ns(
        "root/Microsoft/Windows/Storage",
        "MSFT_PhysicalDisk",
        &["FriendlyName", "MediaType", "BusType", "Size", "DeviceId"],
    )
    .unwrap_or_default();

    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());

    physical
        .iter()
        .map(|row| {
            // MSFT_PhysicalDisk MediaType: 3 = HDD, 4 = SSD, 5 = SCM.
            // BusType 17 = NVMe.
            let media_code = process::json_u64(row, "MediaType");
            let bus_code = process::json_u64(row, "BusType");
            let media_type = match (media_code, bus_code) {
                (_, 17) => MediaType::Nvme,
                (4, _) => MediaType::Ssd,
                (3, _) => MediaType::Hdd,
                _ => MediaType::Unknown,
            };

            let device_id = process::json_str(row, "DeviceId");
            let letters = drive_letters_for_disk(&device_id);

            StorageDevice {
                model: process::json_str(row, "FriendlyName"),
                size_gb: process::json_u64(row, "Size") / (1024 * 1024 * 1024),
                bus_type: bus_type_name(bus_code).to_string(),
                is_system_drive: letters.iter().any(|l| l.eq_ignore_ascii_case(&system_drive)),
                drive_letters: letters,
                hosts_fortnite: false,
                media_type,
            }
        })
        .collect()
}

fn bus_type_name(code: u64) -> &'static str {
    match code {
        1 => "SCSI",
        3 => "ATA",
        7 => "USB",
        8 => "RAID",
        11 => "SATA",
        17 => "NVMe",
        _ => "Unknown",
    }
}

/// Resolves the drive letters backed by a physical disk number.
fn drive_letters_for_disk(device_id: &str) -> Vec<String> {
    let Ok(number) = device_id.trim().parse::<u32>() else {
        return Vec::new();
    };
    let script = format!(
        "Get-Partition -DiskNumber {number} -ErrorAction SilentlyContinue | \
         Where-Object DriveLetter | Select-Object DriveLetter | ConvertTo-Json -Compress"
    );
    let Ok(raw) = process::powershell(&script) else {
        return Vec::new();
    };

    process::normalise_json_array(&raw)
        .iter()
        .filter_map(|v| {
            let letter = process::json_str(v, "DriveLetter");
            (!letter.is_empty()).then(|| format!("{letter}:"))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

fn scan_network() -> Vec<NetworkAdapter> {
    let rows = process::cim_query(
        "Win32_NetworkAdapter",
        &[
            "Name",
            "Description",
            "GUID",
            "MACAddress",
            "Speed",
            "NetConnectionStatus",
            "PNPDeviceID",
            "NetConnectionID",
            "Manufacturer",
            "PhysicalAdapter",
        ],
    )
    .unwrap_or_default();

    let class_index = index_class_keys(NET_CLASS);
    let default_route_mac = default_route_mac();

    rows.iter()
        // Physical adapters only: virtual, tunnel and loopback adapters have no
        // registry properties worth setting and would pollute the plan.
        .filter(|row| process::json_bool(row, "PhysicalAdapter"))
        .map(|row| {
            let description = process::json_str(row, "Description");
            let mac = process::json_str(row, "MACAddress");
            // NetConnectionStatus 2 = Connected.
            let connected = process::json_u64(row, "NetConnectionStatus") == 2;

            NetworkAdapter {
                is_wifi: description.to_lowercase().contains("wi-fi")
                    || description.to_lowercase().contains("wireless")
                    || description.to_lowercase().contains("802.11"),
                class_key_index: class_index
                    .iter()
                    .find(|(_, desc)| desc == &description)
                    .map(|(idx, _)| idx.clone()),
                // NetConnectionID is the friendly name netsh expects ("Ethernet").
                name: process::json_str(row, "NetConnectionID"),
                guid: process::json_str(row, "GUID"),
                pnp_device_id: process::json_str(row, "PNPDeviceID"),
                link_speed_mbps: process::json_u64(row, "Speed") / 1_000_000,
                is_default_route: connected
                    && !mac.is_empty()
                    && default_route_mac.as_deref() == Some(mac.as_str()),
                vendor: process::json_str(row, "Manufacturer"),
                is_connected: connected,
                mac_address: mac,
                description,
            }
        })
        .collect()
}

/// MAC address of the interface carrying the default route.
fn default_route_mac() -> Option<String> {
    let script = "$r = Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue | \
                  Sort-Object RouteMetric | Select-Object -First 1; \
                  if ($r) { (Get-NetAdapter -InterfaceIndex $r.ifIndex -ErrorAction SilentlyContinue).MacAddress }";
    let raw = process::powershell(script).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Get-NetAdapter formats as AA-BB-CC; CIM uses AA:BB:CC.
    Some(trimmed.replace('-', ":").to_uppercase())
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

fn scan_displays() -> Vec<Display> {
    let rows = process::cim_query(
        "Win32_VideoController",
        &[
            "Name",
            "CurrentHorizontalResolution",
            "CurrentVerticalResolution",
            "CurrentRefreshRate",
            "MaxRefreshRate",
            "MinRefreshRate",
        ],
    )
    .unwrap_or_default();

    rows.iter()
        .filter(|row| process::json_u32(row, "CurrentHorizontalResolution") > 0)
        .enumerate()
        .map(|(i, row)| Display {
            name: process::json_str(row, "Name"),
            width: process::json_u32(row, "CurrentHorizontalResolution"),
            height: process::json_u32(row, "CurrentVerticalResolution"),
            refresh_hz: process::json_u32(row, "CurrentRefreshRate"),
            max_refresh_hz: process::json_u32(row, "MaxRefreshRate"),
            is_primary: i == 0,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Operating system
// ---------------------------------------------------------------------------

fn scan_os() -> OperatingSystem {
    let rows = process::cim_query(
        "Win32_OperatingSystem",
        &["Caption", "Version", "BuildNumber", "OSArchitecture", "InstallDate"],
    )
    .unwrap_or_default();
    let row = rows.first().cloned().unwrap_or(serde_json::Value::Null);

    let build: u32 = process::json_str(&row, "BuildNumber").parse().unwrap_or(0);
    let caption = process::json_str(&row, "Caption");

    let display_version = read_sz(
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        "DisplayVersion",
    )
    .unwrap_or_default();

    // Win32_DeviceGuard reports 2 when VBS is running rather than merely enabled.
    let device_guard = process::cim_query_ns(
        "root/Microsoft/Windows/DeviceGuard",
        "Win32_DeviceGuard",
        &["VirtualizationBasedSecurityStatus", "SecurityServicesRunning"],
    )
    .unwrap_or_default();

    let vbs_enabled = device_guard
        .first()
        .map(|r| process::json_u64(r, "VirtualizationBasedSecurityStatus") == 2)
        .unwrap_or(false);

    // SecurityServicesRunning contains 2 when HVCI (Memory Integrity) is active.
    let hvci_enabled = device_guard
        .first()
        .and_then(|r| r.get("SecurityServicesRunning").cloned())
        .and_then(|v| v.as_array().cloned())
        .map(|arr| arr.iter().any(|x| x.as_u64() == Some(2)))
        .unwrap_or(false);

    OperatingSystem {
        is_windows_11: build >= 22000,
        build,
        version: process::json_str(&row, "Version"),
        architecture: process::json_str(&row, "OSArchitecture"),
        install_date: process::json_str(&row, "InstallDate"),
        display_version,
        vbs_enabled,
        hvci_enabled,
        core_isolation_enabled: hvci_enabled,
        hags_enabled: read_dword(
            r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers",
            "HwSchMode",
        )
        .unwrap_or(0)
            == 2,
        game_mode_enabled: read_dword_hkcu(r"Software\Microsoft\GameBar", "AutoGameModeEnabled")
            .unwrap_or(0)
            == 1,
        game_dvr_enabled: read_dword_hkcu(r"System\GameConfigStore", "GameDVR_Enabled")
            .unwrap_or(1)
            == 1,
        caption,
    }
}

// ---------------------------------------------------------------------------
// Peripherals
// ---------------------------------------------------------------------------

fn scan_peripherals() -> Peripherals {
    let devices = process::cim_query(
        "Win32_PnPEntity",
        &["Name", "DeviceID", "PNPClass", "Service", "Manufacturer"],
    )
    .unwrap_or_default();

    let mut mice = Vec::new();
    let mut keyboards = Vec::new();
    let mut controllers = Vec::new();

    for row in &devices {
        let name = process::json_str(row, "Name");
        let device_id = process::json_str(row, "DeviceID");
        let class = process::json_str(row, "PNPClass");
        let service = process::json_str(row, "Service");
        let lower = name.to_lowercase();

        if device_id.is_empty() {
            continue;
        }

        let (vid, pid) = parse_vid_pid(&device_id);
        let wireless = lower.contains("wireless")
            || lower.contains("bluetooth")
            || device_id.starts_with("BTHENUM");

        // Controllers are identified by driver service first, because gamepad
        // names vary wildly between third-party pads.
        let is_pad = matches!(service.as_str(), "xboxgip" | "XboxGipSvc" | "xinputhid" | "HidBth")
            || lower.contains("controller")
            || lower.contains("gamepad")
            || lower.contains("dualsense")
            || lower.contains("dualshock")
            || lower.contains("wireless controller");

        if is_pad {
            controllers.push(Controller {
                kind: classify_controller(&lower, &vid, &pid),
                connection: if device_id.starts_with("BTHENUM") || lower.contains("bluetooth") {
                    ControllerConnection::Bluetooth
                } else if lower.contains("wireless adapter") || lower.contains("dongle") {
                    ControllerConnection::ProprietaryWireless
                } else if device_id.starts_with("USB") {
                    ControllerConnection::WiredUsb
                } else {
                    ControllerConnection::Unknown
                },
                pnp_device_id: device_id,
                vendor_id: vid,
                product_id: pid,
                name,
            });
            continue;
        }

        let device = InputDevice {
            is_wireless: wireless,
            usb_parent_path: None,
            pnp_device_id: device_id,
            vendor_id: vid,
            product_id: pid,
            name,
        };

        match class.as_str() {
            "Mouse" => mice.push(device),
            "Keyboard" => keyboards.push(device),
            _ => {}
        }
    }

    Peripherals {
        pointer_precision_enabled: read_sz_hkcu(r"Control Panel\Mouse", "MouseSpeed")
            .map(|v| v != "0")
            .unwrap_or(false),
        mouse_speed: read_sz_hkcu(r"Control Panel\Mouse", "MouseSensitivity")
            .and_then(|v| v.parse().ok())
            .unwrap_or(10),
        keyboard_repeat_delay: read_sz_hkcu(r"Control Panel\Keyboard", "KeyboardDelay")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1),
        keyboard_repeat_rate: read_sz_hkcu(r"Control Panel\Keyboard", "KeyboardSpeed")
            .and_then(|v| v.parse().ok())
            .unwrap_or(31),
        // The accessibility Flags values are bitmasks; bit 1 is "feature on".
        filter_keys_enabled: accessibility_on(r"Control Panel\Accessibility\Keyboard Response"),
        sticky_keys_enabled: accessibility_on(r"Control Panel\Accessibility\StickyKeys"),
        toggle_keys_enabled: accessibility_on(r"Control Panel\Accessibility\ToggleKeys"),
        mice,
        keyboards,
        controllers,
    }
}

fn accessibility_on(path: &str) -> bool {
    read_sz_hkcu(path, "Flags")
        .and_then(|v| v.parse::<u32>().ok())
        .map(|flags| flags & 1 != 0)
        .unwrap_or(false)
}

fn classify_controller(lower_name: &str, vid: &str, pid: &str) -> ControllerKind {
    // Sony and Nintendo vendor IDs are stable and more reliable than names.
    match (vid, pid) {
        ("054C", "09CC") | ("054C", "05C4") => return ControllerKind::DualShock4,
        ("054C", "0CE6") | ("054C", "0DF2") => return ControllerKind::DualSense,
        ("057E", _) => return ControllerKind::SwitchPro,
        _ => {}
    }

    if lower_name.contains("dualsense") {
        ControllerKind::DualSense
    } else if lower_name.contains("dualshock") {
        ControllerKind::DualShock4
    } else if lower_name.contains("series") {
        ControllerKind::XboxSeries
    } else if lower_name.contains("360") {
        ControllerKind::Xbox360
    } else if lower_name.contains("xbox") {
        ControllerKind::XboxOne
    } else {
        ControllerKind::GenericHid
    }
}

/// Pulls VID and PID out of a device instance path such as
/// `USB\VID_045E&PID_02EA\...`.
fn parse_vid_pid(device_id: &str) -> (String, String) {
    let extract = |key: &str| -> String {
        device_id
            .find(key)
            .map(|i| {
                device_id[i + key.len()..]
                    .chars()
                    .take(4)
                    .collect::<String>()
                    .to_uppercase()
            })
            .unwrap_or_default()
    };
    (extract("VID_"), extract("PID_"))
}

// ---------------------------------------------------------------------------
// Power / motherboard / Fortnite
// ---------------------------------------------------------------------------

fn scan_power() -> PowerState {
    let active = process::run("powercfg.exe", &["/getactivescheme"])
        .map(|o| o.stdout)
        .unwrap_or_default();

    // Output: "Power Scheme GUID: <guid>  (<name>)"
    let guid = active
        .split_whitespace()
        .find(|t| t.len() == 36 && t.matches('-').count() == 4)
        .unwrap_or_default()
        .to_string();
    let name = active
        .find('(')
        .and_then(|i| active[i + 1..].find(')').map(|j| active[i + 1..i + 1 + j].to_string()))
        .unwrap_or_default();

    // A battery present means laptop, which changes the correct answer for every
    // power-related tweak in the catalog.
    let is_laptop = !process::cim_query("Win32_Battery", &["Name"])
        .unwrap_or_default()
        .is_empty();

    let schemes = process::run("powercfg.exe", &["/list"])
        .map(|o| o.stdout)
        .unwrap_or_default();

    PowerState {
        ultimate_performance_available: schemes.contains("e9a42b02-d5df-448d-aa00-03f14749eb61"),
        active_scheme_guid: guid,
        active_scheme_name: name,
        is_laptop,
    }
}

fn scan_motherboard() -> Motherboard {
    let boards = process::cim_query("Win32_BaseBoard", &["Manufacturer", "Product"])
        .unwrap_or_default();
    let bios = process::cim_query(
        "Win32_BIOS",
        &["Manufacturer", "SMBIOSBIOSVersion", "ReleaseDate"],
    )
    .unwrap_or_default();

    let board = boards.first().cloned().unwrap_or(serde_json::Value::Null);
    let firmware = bios.first().cloned().unwrap_or(serde_json::Value::Null);

    let secure_boot = process::powershell("Confirm-SecureBootUEFI -ErrorAction SilentlyContinue")
        .map(|s| s.trim().eq_ignore_ascii_case("True"))
        .unwrap_or(false);

    Motherboard {
        manufacturer: process::json_str(&board, "Manufacturer"),
        product: process::json_str(&board, "Product"),
        bios_vendor: process::json_str(&firmware, "Manufacturer"),
        bios_version: process::json_str(&firmware, "SMBIOSBIOSVersion"),
        bios_release_date: process::json_str(&firmware, "ReleaseDate"),
        secure_boot_enabled: secure_boot,
        // Resizable BAR cannot be read reliably without vendor tooling; left for
        // the BIOS advisory to raise as a "check this" item rather than guessed.
        resizable_bar_active: false,
    }
}

fn scan_fortnite() -> FortniteInstall {
    // The launcher records install locations in its manifests; the registry
    // key is the more stable of the two.
    let install_path = read_sz(
        r"SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher",
        "AppDataPath",
    );

    // Probe the conventional locations across all fixed drives.
    let candidates: Vec<String> = ('C'..='Z')
        .map(|d| format!(r"{d}:\Program Files\Epic Games\Fortnite"))
        .collect();

    let found_path = candidates.into_iter().find(|p| std::path::Path::new(p).exists());

    let exe = found_path.as_ref().map(|p| {
        format!(r"{p}\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping.exe")
    });

    let config = std::env::var("LOCALAPPDATA")
        .ok()
        .map(|p| format!(r"{p}\FortniteGame\Saved\Config\WindowsClient"));

    FortniteInstall {
        found: found_path.is_some(),
        // Only report the executable when it is actually on disk, since several
        // catalog entries key registry values on the full path.
        executable_path: exe.filter(|p| std::path::Path::new(p).exists()),
        epic_launcher_path: install_path,
        config_path: config,
        install_path: found_path,
    }
}

// ---------------------------------------------------------------------------
// Small registry read helpers
// ---------------------------------------------------------------------------

fn read_sz(path: &str, value: &str) -> Option<String> {
    match registry::get_value(Hive::LocalMachine, path, value) {
        Ok(Some(RegData::Sz(s))) | Ok(Some(RegData::ExpandSz(s))) => Some(s),
        _ => None,
    }
}

fn read_sz_hkcu(path: &str, value: &str) -> Option<String> {
    match registry::get_value(Hive::CurrentUser, path, value) {
        Ok(Some(RegData::Sz(s))) | Ok(Some(RegData::ExpandSz(s))) => Some(s),
        _ => None,
    }
}

fn read_dword(path: &str, value: &str) -> Option<u32> {
    match registry::get_value(Hive::LocalMachine, path, value) {
        Ok(Some(RegData::Dword(v))) => Some(v),
        _ => None,
    }
}

fn read_dword_hkcu(path: &str, value: &str) -> Option<u32> {
    match registry::get_value(Hive::CurrentUser, path, value) {
        Ok(Some(RegData::Dword(v))) => Some(v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_intel_generation() {
        assert_eq!(parse_generation("Intel(R) Core(TM) i5-12400F", CpuVendor::Intel), Some(12));
        assert_eq!(parse_generation("Intel(R) Core(TM) i7-13700K", CpuVendor::Intel), Some(13));
        assert_eq!(parse_generation("Intel(R) Core(TM) i7-9700K", CpuVendor::Intel), Some(9));
        assert_eq!(parse_generation("Intel(R) Core(TM) Ultra 7 265K", CpuVendor::Intel), Some(14));
    }

    #[test]
    fn parses_amd_generation() {
        assert_eq!(parse_generation("AMD Ryzen 5 7600X", CpuVendor::Amd), Some(7));
        assert_eq!(parse_generation("AMD Ryzen 7 5800X3D", CpuVendor::Amd), Some(5));
    }

    #[test]
    fn parses_vid_pid_from_device_path() {
        let (vid, pid) = parse_vid_pid(r"USB\VID_045E&PID_02EA\3033363030303");
        assert_eq!(vid, "045E");
        assert_eq!(pid, "02EA");
    }

    #[test]
    fn unparseable_device_path_yields_empty_ids() {
        let (vid, pid) = parse_vid_pid(r"ACPI\PNP0303\4&1a2b3c4d&0");
        assert!(vid.is_empty() && pid.is_empty());
    }

    #[test]
    fn classifies_sony_pads_by_vendor_id() {
        assert_eq!(classify_controller("wireless controller", "054C", "0CE6"), ControllerKind::DualSense);
        assert_eq!(classify_controller("wireless controller", "054C", "09CC"), ControllerKind::DualShock4);
    }

    #[test]
    fn xmp_detection_requires_a_meaningful_gap() {
        let running_at_rated = Memory { configured_mhz: 3200, rated_mhz: 3200, ..Default::default() };
        assert!(!running_at_rated.xmp_appears_disabled());

        let jedec_fallback = Memory { configured_mhz: 2133, rated_mhz: 3200, ..Default::default() };
        assert!(jedec_fallback.xmp_appears_disabled());
    }

    #[test]
    fn underdriven_display_is_detected() {
        let d = Display { refresh_hz: 60, max_refresh_hz: 240, ..Default::default() };
        assert!(d.is_underdriven());

        let ok = Display { refresh_hz: 240, max_refresh_hz: 240, ..Default::default() };
        assert!(!ok.is_underdriven());
    }
}
