//! System.
//!
//! Background services, shell overhead and telemetry. Individually most of these
//! are small; the point is the aggregate, and the removal of things that fire
//! *unpredictably* — a background update check mid-match costs far more than its
//! average CPU usage suggests.
//!
//! Windows Defender is deliberately left running. Disabling real-time protection
//! is the most-recommended "optimisation" on the internet and Forged will not do
//! it: the correct fix is an exclusion for the game directory, which removes the
//! scanning cost on the hot path without leaving the machine unprotected.

use crate::tweaks::model::*;

const EXPLORER_ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const CONTENT_DELIVERY: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";

pub static TWEAKS: &[Tweak] = &[
    Tweak {
        id: "system.enable_game_mode",
        name: "Enable Game Mode",
        section: Section::System,
        summary: "Turns on Windows Game Mode and automatic game detection.",
        rationale: "Game Mode suppresses background maintenance, Windows Update installs and \
                    driver updates while a game is in the foreground. Early versions were \
                    genuinely bad and the advice to disable it stuck around; the current \
                    implementation helps, particularly on lower-core-count CPUs.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hkcu_dword(r"Software\Microsoft\GameBar", "AllowAutoGameMode", 1),
                hkcu_dword(r"Software\Microsoft\GameBar", "AutoGameModeEnabled", 1),
            ]
        },
    },
    Tweak {
        id: "system.disable_telemetry",
        name: "Disable telemetry collection",
        section: Section::System,
        summary: "Stops the Connected User Experiences service and sets telemetry to the minimum \
                  the edition allows.",
        rationale: "DiagTrack wakes periodically to collect and upload diagnostic data. The CPU \
                    cost is small; the unpredictability is the problem, since it does not care \
                    whether you are mid-match.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                service("DiagTrack", ServiceStart::Disabled),
                service("dmwappushservice", ServiceStart::Disabled),
                hklm_dword(
                    r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
                    "AllowTelemetry",
                    0,
                ),
                hklm_dword(
                    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection",
                    "AllowTelemetry",
                    0,
                ),
            ]
        },
    },
    Tweak {
        id: "system.disable_background_apps",
        name: "Stop background apps running",
        section: Section::System,
        summary: "Globally disables background execution for Store applications.",
        rationale:
            "Store apps run background tasks to fetch mail, update live tiles and check for \
                    content. On a machine that only plays Fortnite, none of that is wanted.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: Some("Store app notifications stop arriving until you open the app."),
        applies_to: always,
        build: |_| {
            vec![
                hkcu_dword(
                    r"Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications",
                    "GlobalUserDisabled",
                    1,
                ),
                hkcu_dword(
                    r"Software\Microsoft\Windows\CurrentVersion\Search",
                    "BackgroundAppGlobalToggle",
                    0,
                ),
            ]
        },
    },
    Tweak {
        id: "system.visual_effects_performance",
        name: "Set visual effects to best performance",
        section: Section::System,
        summary: "Disables window animations, shadows, fades and the associated compositor work.",
        rationale: "Every animation is drawn by the desktop compositor, which shares the GPU with \
                    the game. It matters most when alt-tabbing, where the animation competes \
                    directly with the frame the game is trying to present.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: Some("Windows looks plainer. Purely cosmetic."),
        applies_to: always,
        build: |_| {
            vec![
                hklm_dword(
                    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects",
                    "VisualFXSetting",
                    2,
                ),
                hkcu_sz(r"Control Panel\Desktop\WindowMetrics", "MinAnimate", "0"),
                hkcu_dword(r"Software\Microsoft\Windows\DWM", "EnableAeroPeek", 0),
                hkcu_dword(EXPLORER_ADVANCED, "TaskbarAnimations", 0),
                hkcu_dword(EXPLORER_ADVANCED, "ListviewAlphaSelect", 0),
                hkcu_dword(EXPLORER_ADVANCED, "ListviewShadow", 0),
            ]
        },
    },
    Tweak {
        id: "system.disable_transparency",
        name: "Disable window transparency",
        section: Section::System,
        summary: "Turns off the acrylic and mica blur effects.",
        rationale: "Blur is a per-frame GPU shader pass on every translucent surface. Removing it \
                    frees a small amount of GPU time that would otherwise be spent on the desktop.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![hkcu_dword(
                r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                "EnableTransparency",
                0,
            )]
        },
    },
    Tweak {
        id: "system.disable_widgets",
        name: "Disable Widgets and News",
        section: Section::System,
        summary: "Removes the widgets board and its background feed updater.",
        rationale: "The widgets host is a persistent web view that polls for news, weather and \
                    stock updates. It is one of the heaviest background components on a stock \
                    Windows 11 install.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: None,
        applies_to: |p| p.os.is_windows_11,
        build: |_| {
            vec![
                hklm_dword(
                    r"SOFTWARE\Policies\Microsoft\Dsh",
                    "AllowNewsAndInterests",
                    0,
                ),
                hkcu_dword(EXPLORER_ADVANCED, "TaskbarDa", 0),
            ]
        },
    },
    Tweak {
        id: "system.disable_copilot",
        name: "Disable Windows Copilot",
        section: Section::System,
        summary: "Turns off the Copilot assistant and its taskbar entry.",
        rationale: "Another always-resident web view. Unused on a gaming machine.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: |p| p.os.is_windows_11,
        build: |_| {
            vec![
                hkcu_dword(
                    r"Software\Policies\Microsoft\Windows\WindowsCopilot",
                    "TurnOffWindowsCopilot",
                    1,
                ),
                hkcu_dword(EXPLORER_ADVANCED, "ShowCopilotButton", 0),
            ]
        },
    },
    Tweak {
        id: "system.disable_startup_delay",
        name: "Remove the startup program delay",
        section: Section::System,
        summary: "Sets StartupDelayInMSec to zero.",
        rationale: "Windows delays startup programs by about ten seconds to make the desktop \
                    appear responsive sooner. The work still happens — it just happens later, \
                    potentially while you are already launching the game.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![hkcu_dword(
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\Serialize",
                "StartupDelayInMSec",
                0,
            )]
        },
    },
    Tweak {
        id: "system.disable_delivery_optimization",
        name: "Stop peer-to-peer update sharing",
        section: Section::System,
        summary: "Sets Delivery Optimization to download from Microsoft only.",
        rationale: "By default your machine uploads Windows update content to other PCs on the \
                    internet. That is outbound bandwidth competing with your game traffic, which \
                    is exactly the kind of contention that produces ping spikes.",
        risk: Risk::Low,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hklm_dword(
                    r"SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization",
                    "DODownloadMode",
                    0,
                ),
                service("DoSvc", ServiceStart::Manual),
            ]
        },
    },
    Tweak {
        id: "system.disable_unused_services",
        name: "Disable unused system services",
        section: Section::System,
        summary: "Disables Fax, Remote Registry, retail demo, Maps and error reporting services.",
        rationale: "A conservative list chosen because nothing on it has any function on a home \
                    gaming machine. Deliberately excludes the audio, networking, Xbox accessory \
                    and update services that aggressive debloat scripts break.",
        risk: Risk::Medium,
        impact: Impact::Moderate,
        evidence: Evidence::SituationalGain,
        requires_reboot: true,
        tradeoff: Some(
            "If you later connect a printer or use Remote Desktop, some of these need \
                        turning back on. The rollback button restores all of them.",
        ),
        applies_to: always,
        build: |_| {
            vec![
                service("Fax", ServiceStart::Disabled),
                service("RemoteRegistry", ServiceStart::Disabled),
                service("RetailDemo", ServiceStart::Disabled),
                service("MapsBroker", ServiceStart::Disabled),
                service("WerSvc", ServiceStart::Disabled),
                service("PcaSvc", ServiceStart::Disabled),
                service("WMPNetworkSvc", ServiceStart::Disabled),
                service("lfsvc", ServiceStart::Disabled),
                service("SharedAccess", ServiceStart::Disabled),
                service("WalletService", ServiceStart::Disabled),
            ]
        },
    },
    Tweak {
        id: "system.disable_print_spooler",
        name: "Disable the print spooler",
        section: Section::System,
        summary: "Stops the printing service.",
        rationale: "Separated from the list above because it is the one people actually miss. On \
                    a dedicated gaming machine with no printer it is a resident service and a \
                    recurring source of security advisories.",
        risk: Risk::Medium,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some("Printing stops working entirely, including print-to-PDF."),
        applies_to: always,
        build: |_| vec![service("Spooler", ServiceStart::Disabled)],
    },
    Tweak {
        id: "system.defender_game_exclusion",
        name: "Exclude the game folder from real-time scanning",
        section: Section::System,
        summary: "Adds the Fortnite install directory and its executable to Defender's exclusion \
                  list.",
        rationale: "Defender inspects files as they are read. Fortnite reads asset files \
                    constantly, so every read pays a scanning cost. Excluding the game directory \
                    removes that cost on the hot path while leaving real-time protection fully \
                    active everywhere else. This is the correct alternative to the widespread \
                    advice to disable Defender outright, which Forged will not do.",
        risk: Risk::Medium,
        impact: Impact::Major,
        evidence: Evidence::Measured,
        requires_reboot: false,
        tradeoff: Some(
            "Files inside the game folder are no longer scanned in real time. Only \
                        install Fortnite from the official Epic launcher.",
        ),
        applies_to: |p| p.fortnite.found,
        build: |p| {
            let Some(path) = p.fortnite.install_path.as_ref() else {
                return Vec::new();
            };
            vec![Action::RunCommand {
                program: "powershell.exe".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-Command".into(),
                    format!("Add-MpPreference -ExclusionPath '{path}'"),
                ],
                revert: RevertCommand {
                    program: "powershell.exe".into(),
                    args: vec![
                        "-NoProfile".into(),
                        "-Command".into(),
                        format!("Remove-MpPreference -ExclusionPath '{path}'"),
                    ],
                },
                tolerate_exit_codes: vec![1],
            }]
        },
    },
    Tweak {
        id: "system.disable_suggestions",
        name: "Disable suggestions, tips and ads",
        section: Section::System,
        summary: "Turns off the content delivery manager's suggested apps, tips and lock screen \
                  content.",
        rationale: "These fetch content over the network on a schedule and render notifications \
                    that steal focus from a fullscreen game.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hkcu_dword(CONTENT_DELIVERY, "SilentInstalledAppsEnabled", 0),
                hkcu_dword(CONTENT_DELIVERY, "SystemPaneSuggestionsEnabled", 0),
                hkcu_dword(CONTENT_DELIVERY, "SoftLandingEnabled", 0),
                hkcu_dword(CONTENT_DELIVERY, "SubscribedContent-338388Enabled", 0),
                hkcu_dword(CONTENT_DELIVERY, "SubscribedContent-338389Enabled", 0),
                hkcu_dword(CONTENT_DELIVERY, "SubscribedContent-353694Enabled", 0),
            ]
        },
    },
    Tweak {
        id: "system.disable_notifications_in_game",
        name: "Suppress notifications during fullscreen apps",
        section: Section::System,
        summary: "Enables Focus Assist's automatic fullscreen rule.",
        rationale:
            "A toast notification over a fullscreen game forces a compositor transition and \
                    can drop the game out of exclusive fullscreen entirely. Enabling the rule is \
                    strictly better than being interrupted.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hkcu_dword(
                    r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings",
                    "NOC_GLOBAL_SETTING_TOASTS_ENABLED",
                    0,
                ),
                hkcu_dword(
                    r"Software\Microsoft\Windows\CurrentVersion\QuietHours",
                    "Enabled",
                    1,
                ),
            ]
        },
    },
    Tweak {
        id: "system.disable_activity_history",
        name: "Disable activity history and timeline",
        section: Section::System,
        summary: "Stops Windows recording and uploading application usage history.",
        rationale: "Writes to disk on every application switch and syncs to your Microsoft \
                    account. No function on a gaming machine.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            let path = r"SOFTWARE\Policies\Microsoft\Windows\System";
            vec![
                hklm_dword(path, "EnableActivityFeed", 0),
                hklm_dword(path, "PublishUserActivities", 0),
                hklm_dword(path, "UploadUserActivities", 0),
            ]
        },
    },
    Tweak {
        id: "system.disable_hibernate_prefetch_boot",
        name: "Disable Fast Startup",
        section: Section::System,
        summary: "Turns off hybrid boot so a shutdown is a real shutdown.",
        rationale: "Fast Startup hibernates the kernel session instead of shutting down, which \
                    means driver and registry changes do not fully apply on the next boot. This is \
                    the reason a tweak can appear not to work until the third restart.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: true,
        tradeoff: Some("Cold boot takes a few seconds longer."),
        applies_to: always,
        build: |_| {
            vec![hklm_dword(
                r"SYSTEM\CurrentControlSet\Control\Session Manager\Power",
                "HiberbootEnabled",
                0,
            )]
        },
    },
    Tweak {
        id: "system.disable_edge_preload",
        name: "Stop Edge preloading at startup",
        section: Section::System,
        summary: "Disables Edge's startup boost and background preload.",
        rationale: "Edge loads browser processes at boot to make itself open faster, whether or \
                    not you use it. Straight overhead on a machine that plays one game.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: Some("Edge takes a second longer to open."),
        applies_to: always,
        build: |_| {
            let path = r"SOFTWARE\Policies\Microsoft\Edge";
            vec![
                hklm_dword(path, "StartupBoostEnabled", 0),
                hklm_dword(path, "BackgroundModeEnabled", 0),
            ]
        },
    },
    Tweak {
        id: "system.disable_onedrive_startup",
        name: "Remove OneDrive from startup",
        section: Section::System,
        summary: "Stops OneDrive launching with Windows.",
        rationale: "OneDrive scans and syncs in the background, producing disk and network \
                    activity at unpredictable moments. It is not uninstalled — only stopped from \
                    starting automatically.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: Some("Files stop syncing until you launch OneDrive manually."),
        applies_to: always,
        build: |_| {
            vec![Action::DeleteRegistryValue {
                hive: Hive::CurrentUser,
                path: r"Software\Microsoft\Windows\CurrentVersion\Run".into(),
                value: "OneDrive".into(),
            }]
        },
    },
    Tweak {
        id: "system.disable_search_highlights",
        name: "Disable search highlights",
        section: Section::System,
        summary: "Removes the animated content feed from the search box.",
        rationale: "Fetches images and content from the internet on a timer, for a feature nobody \
                    asked for.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![hkcu_dword(
                r"Software\Microsoft\Windows\CurrentVersion\SearchSettings",
                "IsDynamicSearchBoxEnabled",
                0,
            )]
        },
    },
    Tweak {
        id: "system.menu_show_delay",
        name: "Remove the menu open delay",
        section: Section::System,
        summary: "Sets MenuShowDelay to zero.",
        rationale:
            "Purely a responsiveness change to the desktop, not the game. Costs nothing and \
                    makes the machine feel quicker, which is most of what people are chasing when \
                    they ask for a faster PC.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| vec![hkcu_sz(r"Control Panel\Desktop", "MenuShowDelay", "0")],
    },
    Tweak {
        id: "system.disable_sticky_keys_prompt",
        name: "Disable the Sticky Keys prompt",
        section: Section::System,
        summary: "Stops the five-shift-presses accessibility dialog.",
        rationale: "Tapping shift repeatedly is normal in a build fight. The dialog that appears \
                    steals focus and minimises the game.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![hkcu_sz(
                r"Control Panel\Accessibility\StickyKeys",
                "Flags",
                "506",
            )]
        },
    },
    Tweak {
        id: "system.registry_backup_task",
        name: "Disable the periodic registry backup task",
        section: Section::System,
        summary: "Turns off the scheduled RegIdleBackup task.",
        rationale:
            "Runs during idle periods and writes a full copy of the registry hives to disk. \
                    Forged's own journal is a far more precise record of what changed.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::NoMeasuredBenefit,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![command(
                "schtasks.exe",
                &[
                    "/Change",
                    "/TN",
                    r"\Microsoft\Windows\Registry\RegIdleBackup",
                    "/DISABLE",
                ],
                &[
                    "/Change",
                    "/TN",
                    r"\Microsoft\Windows\Registry\RegIdleBackup",
                    "/ENABLE",
                ],
            )]
        },
    },
    Tweak {
        id: "system.disable_customer_experience",
        name: "Disable the Customer Experience Improvement Program",
        section: Section::System,
        summary: "Turns off CEIP scheduled tasks and the application compatibility appraiser.",
        rationale:
            "The appraiser task scans installed software and can run for minutes at a time, \
                    with no regard for what you are doing.",
        risk: Risk::Low,
        impact: Impact::Moderate,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hklm_dword(
                    r"SOFTWARE\Policies\Microsoft\SQMClient\Windows",
                    "CEIPEnable",
                    0,
                ),
                command(
                    "schtasks.exe",
                    &[
                        "/Change",
                        "/TN",
                        r"\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser",
                        "/DISABLE",
                    ],
                    &[
                        "/Change",
                        "/TN",
                        r"\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser",
                        "/ENABLE",
                    ],
                ),
            ]
        },
    },
    Tweak {
        id: "system.taskbar_declutter",
        name: "Remove taskbar search, task view and chat",
        section: Section::System,
        summary: "Hides the search box, Task View button and Chat icon.",
        rationale: "The search box in particular maintains a live connection for web suggestions. \
                    Hiding these removes their backing processes as well as the icons.",
        risk: Risk::Low,
        impact: Impact::Minor,
        evidence: Evidence::Documented,
        requires_reboot: false,
        tradeoff: None,
        applies_to: always,
        build: |_| {
            vec![
                hkcu_dword(
                    r"Software\Microsoft\Windows\CurrentVersion\Search",
                    "SearchboxTaskbarMode",
                    0,
                ),
                hkcu_dword(EXPLORER_ADVANCED, "ShowTaskViewButton", 0),
                hkcu_dword(EXPLORER_ADVANCED, "TaskbarMn", 0),
            ]
        },
    },
];
