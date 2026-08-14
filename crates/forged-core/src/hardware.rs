//! The machine profile that every other subsystem reasons over.
//!
//! The scanner fills this in once at startup; the catalog's applicability
//! predicates read it to decide what is relevant; the AI planner receives it
//! serialised as the sole description of the target machine.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub cpu: Cpu,
    pub gpus: Vec<Gpu>,
    pub memory: Memory,
    pub storage: Vec<StorageDevice>,
    pub network: Vec<NetworkAdapter>,
    pub displays: Vec<Display>,
    pub os: OperatingSystem,
    pub peripherals: Peripherals,
    pub power: PowerState,
    pub fortnite: FortniteInstall,
    pub motherboard: Motherboard,
    /// Wall-clock time the scan completed, for the report header.
    pub scanned_at: String,
}

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cpu {
    pub name: String,
    pub vendor: CpuVendor,
    pub physical_cores: u32,
    pub logical_threads: u32,
    pub base_mhz: u32,
    pub max_mhz: u32,
    /// Intel generation (12 for i5-12400F) or AMD Zen generation, best-effort.
    pub generation: Option<u32>,
    /// True for Intel 12th gen and newer, which ship P-cores and E-cores. Several
    /// scheduler tweaks that help on homogeneous CPUs actively hurt here, so the
    /// catalog gates on this.
    pub hybrid_architecture: bool,
    pub virtualization_enabled: bool,
    pub l3_cache_kb: u32,
    /// AMD 3D V-Cache parts park cores differently; relevant to CPPC tweaks.
    pub has_3d_vcache: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CpuVendor {
    Intel,
    Amd,
    #[default]
    Unknown,
}

impl Cpu {
    /// Intel hybrid parts began at Alder Lake (12th gen).
    pub fn is_intel_hybrid(&self) -> bool {
        self.vendor == CpuVendor::Intel && self.hybrid_architecture
    }

    pub fn is_amd_zen(&self) -> bool {
        self.vendor == CpuVendor::Amd
    }

    /// Low-core-count CPUs benefit from freeing background CPU time far more
    /// than 16-core parts do; several catalog entries weight on this.
    pub fn is_low_core_count(&self) -> bool {
        self.physical_cores > 0 && self.physical_cores <= 6
    }
}

// ---------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Gpu {
    pub name: String,
    pub vendor: GpuVendor,
    pub vram_mb: u64,
    pub driver_version: String,
    pub driver_date: String,
    /// PnP device instance path, needed to locate the device's registry node for
    /// MSI-mode and interrupt-priority tweaks.
    pub pnp_device_id: String,
    /// Registry class key index under CurrentControlSet\...\{4d36e968-...}\NNNN.
    pub class_key_index: Option<String>,
    pub is_primary: bool,
    pub is_integrated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    #[default]
    Unknown,
}

impl Gpu {
    /// Cards below ~6 GB need the texture-streaming and shader-cache tweaks far
    /// more than large-VRAM cards, which mostly want them left alone.
    pub fn is_vram_constrained(&self) -> bool {
        self.vram_mb > 0 && self.vram_mb <= 6144
    }
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Memory {
    pub total_mb: u64,
    /// Speed the modules are actually running at right now.
    pub configured_mhz: u32,
    /// Speed the SPD/XMP profile advertises as the rated maximum.
    pub rated_mhz: u32,
    pub module_count: u32,
    pub modules: Vec<MemoryModule>,
    pub form_factor: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryModule {
    pub manufacturer: String,
    pub part_number: String,
    pub capacity_mb: u64,
    pub configured_mhz: u32,
    pub rated_mhz: u32,
    pub slot: String,
}

impl Memory {
    /// The single highest-value finding the scanner can produce. Running 3200 MT/s
    /// kit at the JEDEC 2133 fallback costs 10-15% of 1% lows in Fortnite, and no
    /// amount of registry work recovers it — it is a BIOS toggle.
    pub fn xmp_appears_disabled(&self) -> bool {
        self.rated_mhz > 0 && self.configured_mhz > 0 && self.rated_mhz > self.configured_mhz + 100
    }

    /// Single-channel configurations lose ~15% average FPS on integrated and
    /// low-end discrete setups. Detected as one populated slot with >= 8 GB.
    pub fn appears_single_channel(&self) -> bool {
        self.module_count == 1 && self.total_mb >= 8192
    }

    pub fn total_gb(&self) -> u64 {
        self.total_mb / 1024
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageDevice {
    pub model: String,
    pub media_type: MediaType,
    pub size_gb: u64,
    pub bus_type: String,
    pub drive_letters: Vec<String>,
    pub is_system_drive: bool,
    /// Set when the Fortnite install was located on this device.
    pub hosts_fortnite: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MediaType {
    Nvme,
    Ssd,
    Hdd,
    #[default]
    Unknown,
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkAdapter {
    pub name: String,
    pub description: String,
    /// Interface GUID, e.g. {A1B2...}. Required to target the per-interface
    /// TcpAckFrequency / TCPNoDelay keys under Tcpip\Parameters\Interfaces.
    pub guid: String,
    /// Registry class key index under {4d36e972-...}\NNNN for advanced NIC
    /// properties (interrupt moderation, offloads, EEE).
    pub class_key_index: Option<String>,
    /// PnP device instance path, needed to reach the adapter's interrupt
    /// management node for MSI mode.
    pub pnp_device_id: String,
    pub mac_address: String,
    pub link_speed_mbps: u64,
    pub is_wifi: bool,
    pub is_connected: bool,
    pub is_default_route: bool,
    pub vendor: String,
}

impl NetworkAdapter {
    /// Wi-Fi adds 3-15 ms of jitter that no registry key removes. The report
    /// surfaces this as a hardware recommendation rather than a tweak.
    pub fn is_latency_compromised(&self) -> bool {
        self.is_wifi && self.is_default_route
    }
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Display {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub max_refresh_hz: u32,
    pub is_primary: bool,
}

impl Display {
    /// A 240 Hz panel running at 60 Hz because Windows defaulted there is a
    /// common and enormous miss.
    pub fn is_underdriven(&self) -> bool {
        self.max_refresh_hz > 0 && self.refresh_hz + 5 < self.max_refresh_hz
    }
}

// ---------------------------------------------------------------------------
// OS
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperatingSystem {
    pub caption: String,
    pub version: String,
    /// e.g. 26100 for Windows 11 24H2.
    pub build: u32,
    pub display_version: String,
    pub architecture: String,
    pub install_date: String,
    /// Virtualisation Based Security. Costs 5-15% CPU-bound FPS when on.
    pub vbs_enabled: bool,
    /// Hypervisor-Enforced Code Integrity, the expensive half of VBS.
    pub hvci_enabled: bool,
    pub hags_enabled: bool,
    pub game_mode_enabled: bool,
    pub game_dvr_enabled: bool,
    pub core_isolation_enabled: bool,
    pub is_windows_11: bool,
}

impl OperatingSystem {
    /// 24H2 changed several scheduler and MPO behaviours; a few catalog entries
    /// only make sense on or before 23H2.
    pub fn is_24h2_or_newer(&self) -> bool {
        self.build >= 26100
    }
}

// ---------------------------------------------------------------------------
// Peripherals — the Controller and KBM sections read from here
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Peripherals {
    pub mice: Vec<InputDevice>,
    pub keyboards: Vec<InputDevice>,
    pub controllers: Vec<Controller>,
    /// Windows "Enhance pointer precision" — mouse acceleration. Must be off.
    pub pointer_precision_enabled: bool,
    pub mouse_speed: u32,
    pub keyboard_repeat_delay: u32,
    pub keyboard_repeat_rate: u32,
    pub filter_keys_enabled: bool,
    pub sticky_keys_enabled: bool,
    pub toggle_keys_enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputDevice {
    pub name: String,
    pub vendor_id: String,
    pub product_id: String,
    pub pnp_device_id: String,
    pub is_wireless: bool,
    /// USB device instance path used to disable selective-suspend per device.
    pub usb_parent_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Controller {
    pub name: String,
    pub kind: ControllerKind,
    pub vendor_id: String,
    pub product_id: String,
    pub pnp_device_id: String,
    pub connection: ControllerConnection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ControllerKind {
    XboxOne,
    XboxSeries,
    Xbox360,
    DualShock4,
    DualSense,
    SwitchPro,
    GenericXInput,
    #[default]
    GenericHid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ControllerConnection {
    WiredUsb,
    Bluetooth,
    /// Xbox Wireless Adapter / proprietary 2.4 GHz dongle.
    ProprietaryWireless,
    #[default]
    Unknown,
}

impl Controller {
    /// Bluetooth adds roughly 8-12 ms over wired on the same pad. Worth telling
    /// the player even though no software tweak fixes it.
    pub fn has_avoidable_latency(&self) -> bool {
        self.connection == ControllerConnection::Bluetooth
    }
}

// ---------------------------------------------------------------------------
// Power
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PowerState {
    pub active_scheme_guid: String,
    pub active_scheme_name: String,
    pub is_laptop: bool,
    pub ultimate_performance_available: bool,
}

// ---------------------------------------------------------------------------
// Motherboard / firmware — feeds the BIOS advisory sheet
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Motherboard {
    pub manufacturer: String,
    pub product: String,
    pub bios_vendor: String,
    pub bios_version: String,
    pub bios_release_date: String,
    pub secure_boot_enabled: bool,
    pub resizable_bar_active: bool,
}

// ---------------------------------------------------------------------------
// Fortnite
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FortniteInstall {
    pub found: bool,
    pub install_path: Option<String>,
    pub executable_path: Option<String>,
    /// %LOCALAPPDATA%\FortniteGame\Saved\Config\WindowsClient
    pub config_path: Option<String>,
    pub epic_launcher_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Derived summary used in report headers and AI prompts
// ---------------------------------------------------------------------------

impl HardwareProfile {
    /// The discrete GPU if there is one, else the first GPU present.
    pub fn primary_gpu(&self) -> Option<&Gpu> {
        self.gpus
            .iter()
            .find(|g| !g.is_integrated)
            .or_else(|| self.gpus.first())
    }

    pub fn primary_display(&self) -> Option<&Display> {
        self.displays
            .iter()
            .find(|d| d.is_primary)
            .or_else(|| self.displays.first())
    }

    /// The adapter carrying the default route — the one whose latency matters.
    pub fn active_adapter(&self) -> Option<&NetworkAdapter> {
        self.network
            .iter()
            .find(|n| n.is_default_route)
            .or_else(|| self.network.iter().find(|n| n.is_connected))
    }

    pub fn system_drive(&self) -> Option<&StorageDevice> {
        self.storage.iter().find(|d| d.is_system_drive)
    }

    /// One-line description used as the report title and AI prompt header.
    pub fn summary_line(&self) -> String {
        let gpu = self
            .primary_gpu()
            .map(|g| g.name.as_str())
            .unwrap_or("unknown GPU");
        let hz = self
            .primary_display()
            .map(|d| d.refresh_hz)
            .unwrap_or_default();
        format!(
            "{} · {} · {} GB RAM @ {} MT/s · {} Hz",
            self.cpu.name.trim(),
            gpu,
            self.memory.total_gb(),
            self.memory.configured_mhz,
            hz
        )
    }

    /// Hardware-level problems that no registry tweak can fix. These are
    /// surfaced at the very top of the report because they dwarf everything
    /// the software side can achieve.
    pub fn blocking_findings(&self) -> Vec<Finding> {
        let mut out = Vec::new();

        if self.memory.xmp_appears_disabled() {
            out.push(Finding {
                severity: Severity::Critical,
                title: "RAM is running below its rated speed".into(),
                detail: format!(
                    "Modules are rated for {} MT/s but are running at {} MT/s. This is the \
                     JEDEC fallback — the XMP/EXPO profile is off in BIOS. Enabling it is worth \
                     more 1% - low FPS than every software tweak in this app combined.",
                    self.memory.rated_mhz, self.memory.configured_mhz
                ),
                fix_location: FixLocation::Bios,
            });
        }

        if self.memory.appears_single_channel() {
            out.push(Finding {
                severity: Severity::Critical,
                title: "Memory appears to be single-channel".into(),
                detail: "Only one memory module was detected. Adding a second matching module \
                         to run in dual-channel typically gains 10-20% average FPS."
                    .into(),
                fix_location: FixLocation::Hardware,
            });
        }

        if let Some(d) = self.primary_display() {
            if d.is_underdriven() {
                out.push(Finding {
                    severity: Severity::Critical,
                    title: "Monitor is not running at its maximum refresh rate".into(),
                    detail: format!(
                        "Primary display is set to {} Hz but supports {} Hz. Forged will correct \
                         this, but verify it holds after reboot.",
                        d.refresh_hz, d.max_refresh_hz
                    ),
                    fix_location: FixLocation::Software,
                });
            }
        }

        if let Some(n) = self.active_adapter() {
            if n.is_latency_compromised() {
                out.push(Finding {
                    severity: Severity::High,
                    title: "Playing over Wi-Fi".into(),
                    detail: "The default route is a wireless adapter. Wi-Fi introduces 3-15 ms of \
                             jitter that no software setting removes. A wired connection is the \
                             single biggest ping improvement available."
                        .into(),
                    fix_location: FixLocation::Hardware,
                });
            }
        }

        if self.os.vbs_enabled {
            out.push(Finding {
                severity: Severity::High,
                title: "Virtualisation Based Security is active".into(),
                detail: "VBS/HVCI costs 5-15% CPU-bound FPS. Forged can disable it, which \
                         measurably reduces the machine's security posture — acceptable on a \
                         dedicated gaming box, not on a general-purpose PC."
                    .into(),
                fix_location: FixLocation::Software,
            });
        }

        for d in &self.storage {
            if d.hosts_fortnite && d.media_type == MediaType::Hdd {
                out.push(Finding {
                    severity: Severity::High,
                    title: "Fortnite is installed on a mechanical hard drive".into(),
                    detail: "Texture streaming from a HDD causes the traversal stutter that no \
                             registry tweak fixes. Moving the install to an SSD or NVMe drive is \
                             the fix."
                        .into(),
                    fix_location: FixLocation::Hardware,
                });
            }
        }

        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub fix_location: FixLocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixLocation {
    /// Forged can fix it.
    Software,
    /// The user must change it in firmware; goes on the BIOS sheet.
    Bios,
    /// Requires buying or replugging something.
    Hardware,
}
